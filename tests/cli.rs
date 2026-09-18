//! The CLI's early exit on `conform --cases`, exercised the way a harness
//! actually calls it: through the built binary, not through the library.
//!
//! `tests/hosts/worker-negotiation.mjs` and `tests/hosts/serve-negotiation.py`
//! both depend on `iyo conform <local-dir> --cases PATH` resolving the
//! contract and writing the plan *without making any HTTP request*, so that
//! a harness can call it with nothing listening on any port. `src/cli.rs`
//! implements this as an early return before `origin` is even computed;
//! nothing in the unit tests around `conform` exercises the CLI layer at
//! all, so a future
//! reordering of that match arm could silently restore the request path and
//! every other test in this crate would keep passing. This spawns
//! `CARGO_BIN_EXE_iyo` the way a harness does, against a target with no
//! server behind it, and would fail if that guarantee broke: with the guard
//! gone this would either try to reach the manifest's own origin
//! (`https://example.org`, reachable from nowhere this test runs) or report
//! findings against an unreachable host, neither of which exits 0 quickly.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::{Ctx, manifest};
use iyo::site::Plan;
use iyo::{build, load, profile, render, serve};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A built copy of `testdata/mini`, the fixture every other integration test
/// in this crate already builds. Built through the library, not through the
/// binary: this test's subject is the CLI's `--cases` guard specifically,
/// and building the fixture through the same subprocess it is about to
/// check would only make failures harder to diagnose.
fn built_fixture() -> Utf8PathBuf {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &root).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    // `render::render` writes `manifest.json` itself, which is all
    // `manifest_of_target` in `src/cli.rs` needs to find below.
    let output = render::render(&ctx).unwrap();

    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-cases-early-exit-site");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    output.write(&dir).unwrap();
    dir
}

#[test]
fn conform_cases_against_a_local_target_exits_clean_with_no_server_anywhere() {
    let site = built_fixture();
    let cases = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-cases-early-exit.json");
    let _ = std::fs::remove_file(&cases);

    // Nothing is listening on example.org from this process, on 127.0.0.1,
    // or anywhere else: the point of the guard is that this command never
    // tries to find out.
    //
    // This used to be measured as "finished quickly", which is a claim about
    // the machine rather than about the command, and it failed on a machine
    // stall that made a 0.02s run take 17s. The hole is the claim itself:
    // every proxy variable curl consults points at it, so a guard that stops
    // short-circuiting routes its request here -- to `https://example.org` or
    // to anywhere else -- and gets counted. A stalled machine cannot
    // manufacture a connection and a fast one cannot hide one.
    let hole = spawn_black_hole();
    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["conform", site.as_str(), "--cases", cases.as_str()])
        .env("http_proxy", hole.url())
        .env("https_proxy", hole.url())
        .env("ALL_PROXY", hole.url())
        .output()
        .expect("spawning the built binary");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        hole.accepted(),
        0,
        "the early exit opened a connection; it must never touch the network"
    );

    // The control for that zero. Same binary, same fixture, same proxy
    // variables, `--cases` dropped: the only difference is the guard, and
    // without it the requests land on the hole. Measured at 40 connections
    // where the guarded run makes none. Without this the assertion above
    // would also pass against a hole nothing could ever have reached -- a
    // typo in a proxy variable, a curl that ignores them -- and would go on
    // passing after the guard it exists to protect had been deleted.
    let control = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "conform",
            site.as_str(),
            "--timeout",
            "1",
            "--deadline",
            "1",
        ])
        .env("http_proxy", hole.url())
        .env("https_proxy", hole.url())
        .env("ALL_PROXY", hole.url())
        .output()
        .expect("spawning the built binary");
    assert!(
        hole.accepted() > 0,
        "nothing reached the hole even with the guard out of the way, so the \
         assertion above proved nothing. control exit {:?}, stderr: {}",
        control.status.code(),
        String::from_utf8_lossy(&control.stderr)
    );

    let text = std::fs::read_to_string(&cases).expect("the plan was written");
    let plan: serde_json::Value = serde_json::from_str(&text).expect("the plan is valid JSON");
    let written_cases = plan["cases"]
        .as_array()
        .expect("a plan carries a cases array");
    assert!(
        !written_cases.is_empty(),
        "the plan resolved against testdata/mini should not be empty"
    );
    assert_eq!(
        plan["unresolved"].as_array().map(Vec::len),
        Some(0),
        "every role in the bundled contract resolves against testdata/mini"
    );
    // Every case the plan wrote carries the fields the local harnesses read.
    for case in written_cases {
        for field in ["name", "group", "namespace", "path", "expect"] {
            assert!(case.get(field).is_some(), "case missing {field}: {case}");
        }
    }
}

/// Build the `mini` fixture into a directory of its own, with a manifest
/// `iyo serve` can serve. Distinct from `built_fixture` above: that one
/// exists to prove a guard that never touches the network; the tests below
/// deliberately run `probe`/`conform` against a live server this process
/// starts, so nothing here reaches the real network either.
fn built_and_served(name: &str) -> (Utf8PathBuf, manifest::Manifest) {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &root).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();
    let manifest = manifest::build(&ctx);

    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join(format!("iyo-cli-live-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    output.write(&dir).unwrap();
    (dir, manifest)
}

/// A listener that accepts every connection and then never answers: the
/// "accepts connections and never answers" host FINDING 1 was measured
/// against, not the "connection refused" a closed port gives you. Each
/// accepted connection is held open, unanswered, well past anything these
/// tests wait for, so `curl`'s own `--timeout` is what ends it.
///
/// It also counts. `accepted()` is what lets a test ask "had anything
/// connected yet?" instead of "how long did that take?", and the difference
/// is the whole point: a stalled machine delays the tool and the hole
/// together, so an order between them survives a stall that any wall-clock
/// bound reads as a failure. Five tests in this file were measured failing
/// together on one ~17s machine stall they all paid and only the ones
/// holding a stopwatch could see.
struct BlackHole {
    port: u16,
    accepted: Arc<AtomicUsize>,
    longest_ms: Arc<AtomicUsize>,
}

impl BlackHole {
    /// `http://127.0.0.1:<port>`, the form every caller here wants.
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// How many connections have been accepted so far.
    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// The same counter, for sampling inside a reader thread.
    fn counter(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.accepted)
    }

    /// How long the longest-held connection lasted before its peer gave up.
    ///
    /// This is curl's `--max-time` observed from the far end. curl enforces
    /// that bound itself, in wall-clock, so the figure is a property of the
    /// flag the tool passed and not of how busy the machine was.
    fn longest_held(&self) -> Duration {
        Duration::from_millis(self.longest_ms.load(Ordering::SeqCst) as u64)
    }
}

/// Waits until the hole has been connected to at least once.
///
/// Every `accepted() == 0` assertion in this file is vacuous unless the run
/// would otherwise have connected, and an assertion that cannot fail is the
/// failure this codebase has now recorded three times. This is the other
/// half of each of those checks. It is a clock, but only a liveness one: a
/// stalled machine makes it poll longer, and the only thing that makes it
/// return false is a connection that never happens at all.
fn connected_within(hole: &BlackHole, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if hole.accepted() > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn spawn_black_hole() -> BlackHole {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let longest_ms = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    let longest = Arc::clone(&longest_ms);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Count before spawning the thread that holds the connection:
            // the question asked of this counter is whether a connection was
            // *made*, and the answer must not wait on a thread to start.
            counter.fetch_add(1, Ordering::SeqCst);
            let longest = Arc::clone(&longest);
            std::thread::spawn(move || {
                let started = Instant::now();
                // Never answer, and never hang up first: read the request and
                // then keep reading, so the connection stays open until the
                // peer abandons it. That moment is curl giving up at its own
                // `--max-time`, which is what makes the duration worth
                // recording. The read timeout only stops this thread from
                // outliving the test process on a peer that never closes.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let mut sink = [0u8; 1024];
                while let Ok(n) = stream.read(&mut sink) {
                    if n == 0 {
                        break;
                    }
                }
                let held = started.elapsed().as_millis() as usize;
                longest.fetch_max(held, Ordering::SeqCst);
                drop(stream);
            });
        }
    });
    BlackHole {
        port,
        accepted,
        longest_ms,
    }
}

#[test]
fn probe_prints_a_header_within_100ms_and_stops_at_its_deadline_against_a_black_hole() {
    let (dir, _manifest) = built_and_served("deadline");
    let hole = spawn_black_hole();

    let mut child = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "probe",
            dir.as_str(),
            "--origin",
            &hole.url(),
            // 30s so that a run which waited on even one request could not
            // print anything for half a minute. The header is checked
            // against the hole's accept count rather than against a clock,
            // and this is what makes that count decisive: the regression
            // leaves 30s of connected silence, not microseconds of it.
            "--timeout",
            "30",
            "--deadline",
            "3",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning the built binary");

    let stderr = child.stderr.take().expect("piped stderr");
    let started = Instant::now();
    let counter = hole.counter();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Keep draining stderr to EOF even after the first line is sent:
        // stopping early would close the pipe's read end while the child is
        // still writing to it (progress, then the report), and a broken
        // pipe there is a write error this process did not cause.
        let mut sent = false;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if !sent {
                // Sampled here rather than in the assertion below so the
                // line and the count are one observation: read them apart
                // and the scheduler is free to insert the whole run between.
                let _ = tx.send((line, counter.load(Ordering::SeqCst)));
                sent = true;
            }
        }
    });

    // The production requirement (clig.dev G22) is that this header precedes
    // any network wait. That is an order, and it is now checked as one: the
    // header arrives with nothing yet connected to the origin. A machine
    // stall delays the tool and the hole alike, so it cannot fail this the
    // way it failed the 10s bound this replaces; a run that waited first
    // cannot pass it, because waiting means connecting.
    let (first_line, connected_when_header_printed) = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("no line ever appeared on stderr");
    assert_eq!(
        connected_when_header_printed, 0,
        "the origin was contacted before the header was printed: {first_line:?}"
    );
    assert!(
        first_line.contains("requests planned"),
        "expected a header naming the plan, got: {first_line:?}"
    );

    // A liveness backstop, not a measurement: it only has to be shorter
    // than a run that ignored `--deadline` entirely and longer than any
    // stall. That the deadline actually clamps each batch is the subject of
    // `conform_does_not_let_a_batch_outrun_the_deadline`, which is
    // parameterised so its own bound has room on both sides.
    let output = child
        .wait_with_output()
        .expect("waiting for the process to exit");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(60),
        "took {elapsed:?}; the run ignored its 3s deadline"
    );
    assert_eq!(output.status.code(), Some(1), "an unreachable origin fails");
    assert!(
        hole.accepted() > 0,
        "the origin was never contacted at all, so the header check above \
         compared nothing against nothing"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The reproduction above targets a local directory, so `manifest_of_target`
/// never blocks on the network before printing anything -- it never
/// surfaced this. `iyo probe <url>` fetches `manifest.json` from the target
/// itself first (`probe::fetch_manifest`, called from `manifest_of_target`
/// in `src/cli.rs`), and that call blocks for up to `--timeout` seconds
/// *before* `probe::run` ever gets a chance to print its own header. Against
/// an unresponsive host that is exactly the silence clig.dev G22 forbids.
#[test]
fn probe_announces_the_target_before_blocking_on_the_manifest_fetch() {
    let hole = spawn_black_hole();
    let target = hole.url();

    let mut child = Command::new(env!("CARGO_BIN_EXE_iyo"))
        // 60s, so that "the announcement came before the fetch blocked"
        // and "the announcement came after it" are a minute apart rather
        // than seconds apart. The child is killed below either way.
        .args(["probe", &target, "--timeout", "60"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning the built binary");

    let stderr = child.stderr.take().expect("piped stderr");
    let counter = hole.counter();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if !sent {
                let _ = tx.send((line, counter.load(Ordering::SeqCst)));
                sent = true;
            }
        }
    });

    // "Before the fetch blocks" is an order, so it is read as one: the
    // announcement arrives with nothing yet connected to the target. The
    // fetch cannot block without first connecting, and the hole is the only
    // thing it can connect to, so a run that fetched first is caught no
    // matter how fast or slow the machine is. The timeout below is a
    // liveness backstop and nothing more: it sits well under the 60s the
    // fetch would otherwise hold the announcement for.
    let (first_line, connected_when_announced) = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("no line ever appeared on stderr");
    assert_eq!(
        connected_when_announced, 0,
        "the target was contacted before it was announced: {first_line:?}"
    );
    assert!(
        first_line.contains(&target),
        "expected the target named in the announcement, got: {first_line:?}"
    );
    assert!(
        connected_within(&hole, Duration::from_secs(30)),
        "the fetch never reached the target, so the zero above proved nothing"
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn conform_announces_with_conform_prefix_not_probe() {
    let hole = spawn_black_hole();

    // There is no clock in this test, because its subject has no duration in
    // it: the prefix on one line is either right or wrong. What the previous
    // version needed a stopwatch for was staying bounded, and the tool's own
    // flags do that better -- at `--timeout 1 --deadline 1` the fetch against
    // the hole gives up in about a second and the process exits by itself,
    // which is what makes `.output()` safe here where the two tests above
    // must hold a child open and kill it. Announcing *before* the fetch is
    // the neighbouring test's subject, not this one's.
    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["conform", &hole.url(), "--timeout", "1", "--deadline", "1"])
        .output()
        .expect("spawning the built binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let first_line = stderr.lines().next().unwrap_or_default();
    assert!(
        first_line.starts_with("conform:"),
        "conform must announce with 'conform:' prefix, got: {first_line:?}"
    );
}

/// A closed pipe ends the process the way it ends every other Unix tool,
/// rather than panicking.
///
/// The Rust runtime sets `SIGPIPE` to `SIG_IGN` before `main`, so a write to
/// a closed pipe returns `EPIPE` and `println!`/`eprintln!` panic on it. The
/// measured result of `iyo build … --pdf 2>&1 | head -4` was **exit 101 with
/// no message at all**, because the panic text goes to the pipe that just
/// closed, and a site holding 2 of its 11 PDFs. 101 is undocumented here and
/// indistinguishable from a real crash.
///
/// 141 is 128 + SIGPIPE, which is what `yes | head` gives and what a shell
/// already explains. The truncation itself is not a defect and cannot be
/// fixed: `| head` asks the producer to stop. What is fixed is that stopping
/// no longer looks like a crash.
#[test]
fn a_closed_pipe_exits_141_rather_than_panicking() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iyo"))
        // `model --json` rather than `build --json`: the build's report is
        // 238 bytes and fits entirely in the 64 KiB pipe buffer, so the
        // child finishes writing and exits 0 before a reader could close
        // anything. The model is 25 KiB and is written in pieces.
        .args(["model", "testdata/mini", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning iyo");

    // Close the read end without reading. Closing is what makes a write
    // fail; merely not reading would just fill the buffer and block.
    drop(child.stdout.take().expect("piped stdout"));
    let status = child.wait().expect("waiting for iyo");

    // 141 when the signal did it, and `None` from `code()` because the
    // process was terminated by a signal rather than exiting. Either shape
    // is the conventional one; 101 is the panic this test exists to keep out.
    let code = status.code();
    assert_ne!(
        code,
        Some(101),
        "a closed pipe still panics; that is exit 101 with no message, \
         because the panic text goes to the pipe that just closed"
    );
    assert!(
        code.is_none() || code == Some(141),
        "expected death by SIGPIPE (or 141), got {code:?}"
    );
}

/// `iyo probe dist --origin … --deadline 0` makes zero requests (the
/// deadline check runs before the first batch), so `errors` and `warnings`
/// are both 0 and `findings` is empty -- but nothing was actually checked.
/// A case that silently did not run must not exit clean, the same line
/// `conform`'s `conformant()` already draws (`src/conform/mod.rs`).
/// `--origin` points at a closed local port so this never depends on
/// anything actually answering there: at `--deadline 0` it must not even
/// try.
#[test]
fn a_truncated_probe_with_no_findings_fails_rather_than_exiting_clean() {
    let (dir, _manifest) = built_and_served("deadline-zero");

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "probe",
            dir.as_str(),
            "--origin",
            "http://127.0.0.1:9",
            "--deadline",
            "0",
        ])
        .output()
        .expect("spawning the built binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a truncated run with zero requests made must not exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Every IRI resolved"),
        "claimed success despite truncation: {stderr}"
    );
    // The same zero-evidence run must not print the FOOPS! probes as a
    // fabricated all-pass either: a probe with no findings and no chance
    // to gather any is "not measured", not "pass".
    for probe in ["URI1", "CN1", "RDF1", "VER2"] {
        assert!(
            stderr.contains(&format!("  {probe:<6} not measured")),
            "{probe} should be reported not measured, not pass, with zero requests made: {stderr}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn probe_report_moves_to_stderr_and_quiet_suppresses_it_while_json_stays_on_stdout() {
    let (dir, manifest) = built_and_served("streams");
    let (port, _thread) = serve::spawn(&dir, manifest).expect("spawn iyo serve");
    let origin = format!("http://127.0.0.1:{port}");

    // Plain human report: nothing on stdout, the report on stderr.
    let plain = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["probe", dir.as_str(), "--origin", &origin])
        .output()
        .expect("running probe");
    assert!(
        plain.stdout.is_empty(),
        "stdout carried human output: {:?}",
        String::from_utf8_lossy(&plain.stdout)
    );
    let plain_stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(
        plain_stderr.contains("errors") && plain_stderr.contains("warnings"),
        "the report did not reach stderr: {plain_stderr}"
    );

    // `--quiet` suppresses the report but not the immediate header.
    let quiet = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["probe", dir.as_str(), "--origin", &origin, "--quiet"])
        .output()
        .expect("running probe --quiet");
    assert!(quiet.stdout.is_empty());
    let quiet_stderr = String::from_utf8_lossy(&quiet.stderr);
    assert!(
        !quiet_stderr.contains("FOOPS!"),
        "--quiet did not suppress the summary: {quiet_stderr}"
    );
    assert!(
        quiet_stderr.contains("requests planned"),
        "--quiet suppressed the responsiveness header too: {quiet_stderr}"
    );

    // `--json`: stdout alone is parseable, whatever `--quiet` says.
    let json = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["probe", dir.as_str(), "--origin", &origin, "--json"])
        .output()
        .expect("running probe --json");
    let parsed: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("stdout was not parseable JSON");
    assert!(parsed.get("origin").is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_userinfo_origin_is_redacted_everywhere_probe_and_conform_report_it() {
    let (dir, manifest) = built_and_served("redaction");
    let (port, _thread) = serve::spawn(&dir, manifest).expect("spawn iyo serve");
    let secret = "hunter2";
    let origin = format!("http://user:{secret}@127.0.0.1:{port}");

    for subcommand in ["probe", "conform"] {
        let json = Command::new(env!("CARGO_BIN_EXE_iyo"))
            .args([subcommand, dir.as_str(), "--origin", &origin, "--json"])
            .output()
            .unwrap_or_else(|e| panic!("running {subcommand} --json: {e}"));
        let stdout = String::from_utf8_lossy(&json.stdout);
        let stderr = String::from_utf8_lossy(&json.stderr);
        assert!(
            !stdout.contains(secret),
            "{subcommand} --json leaked the password on stdout: {stdout}"
        );
        assert!(
            !stderr.contains(secret),
            "{subcommand} --json leaked the password on stderr: {stderr}"
        );
        // Redacted, not merely dropped: the username survives so a reader
        // can tell which credential was used.
        assert!(
            stdout.contains("user:***@"),
            "{subcommand} --json did not carry a redacted origin: {stdout}"
        );

        let human = Command::new(env!("CARGO_BIN_EXE_iyo"))
            .args([subcommand, dir.as_str(), "--origin", &origin])
            .output()
            .unwrap_or_else(|e| panic!("running {subcommand}: {e}"));
        let human_stdout = String::from_utf8_lossy(&human.stdout);
        let human_stderr = String::from_utf8_lossy(&human.stderr);
        assert!(
            !human_stdout.contains(secret) && !human_stderr.contains(secret),
            "{subcommand}'s human summary leaked the password: stdout={human_stdout} stderr={human_stderr}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `--dry-run` runs `build` for its findings without writing anything (clig.dev
/// G23). This is a guard against the atomic stage-and-swap added to
/// `Output::write` for the same audit finding: that write now creates a
/// sibling staging directory before it creates `out_dir` itself, so the
/// `if *dry_run { .. } else { output.write(out)? }` guard in `src/cli.rs`
/// must keep short-circuiting before `write` is ever called, not just before
/// `out_dir` is populated.
#[test]
fn dry_run_creates_no_output_directory_and_no_staging_directory() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mini = root.join("testdata/mini");
    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-dry-run-test");
    let _ = std::fs::remove_dir_all(&out);

    let result = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            mini.as_str(),
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
            "--dry-run",
            "-q",
        ])
        .output()
        .expect("spawning the built binary");
    assert!(
        result.status.success(),
        "exit {:?}, stderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    );

    assert!(!out.exists(), "dry run created the output directory");

    let parent = out.parent().expect("out has a parent");
    let prefix = format!("{}.iyo-", out.file_name().expect("out has a name"));
    let leftovers: Vec<_> = std::fs::read_dir(parent)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .collect();
    assert!(
        leftovers.is_empty(),
        "dry run left a staging directory behind: {leftovers:?}"
    );
}

/// The deadline check runs *before* each batch, not during one, so an
/// unclamped batch's slowest chunk could run for `chunk_depth × timeout`
/// before the next check ever happens (`src/http.rs`: `BATCH = 40`,
/// `CONCURRENCY = 8`, so a first batch of `BATCH` cases chains 5 requests
/// deep). At the defaults that is 75s, above conform's own 60s default
/// deadline, so a slow-but-working origin could truncate on its very first
/// batch. `http::clamped_timeout` clamps each batch's per-request
/// `--max-time` to what remains of the deadline, so a black hole gets
/// stopped close to `--deadline` regardless of how generous `--timeout` is.
#[test]
fn conform_does_not_let_a_batch_outrun_the_deadline() {
    let (dir, _manifest) = built_and_served("deadline-clamp");
    let hole = spawn_black_hole();

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "conform",
            dir.as_str(),
            "--origin",
            &hole.url(),
            // 30, not 5, and the gap is the whole repair. Clamped, every
            // request gets one second: two seconds of deadline spread over a
            // chunk five deep, floored at one. Unclamped, every request gets
            // the full thirty. At 5 the two outcomes were ~5s and ~25s, and
            // this suite's own noise was measured at 22.4s -- signal and
            // noise overlapped, so no bound could separate them and the test
            // reported a stalled machine as a broken clamp.
            "--timeout",
            "30",
            "--deadline",
            "2",
        ])
        .output()
        .expect("spawning the built binary");
    let elapsed = started.elapsed();

    // The clamp is read off the wire, not off the wall clock.
    //
    // What this test protects is the wiring: that `conform` hands
    // `deadline - elapsed` to `http::clamped_timeout` and that the result
    // reaches curl's `--max-time`. The arithmetic itself is unit-tested in
    // `src/http.rs`; delete the subtraction there and every other test in
    // this file still passes.
    //
    // curl enforces `--max-time` itself, in wall-clock, so a clamped request
    // hangs up after about a second however busy the machine is, while an
    // unclamped one holds on for thirty. Reading that duration from the far
    // end of the socket is the one measurement here that a stalled machine
    // cannot move -- which is precisely what the elapsed-time bound it
    // replaces could not say for itself.
    assert!(
        hole.accepted() > 0,
        "nothing ever reached the origin, so there was no clamp to observe"
    );
    let longest = hole.longest_held();
    assert!(
        longest < Duration::from_secs(10),
        "a request was allowed {longest:?} against a 2s deadline; the clamp \
         never reached curl's --max-time. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A liveness backstop on top, so a run that hangs fails instead of
    // sitting there. It is deliberately far from both outcomes.
    assert!(
        elapsed < Duration::from_secs(90),
        "took {elapsed:?}; the run never finished"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "a truncated run must not exit 0"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Gates that were reported as passing while something was wrong.
//
// Each of these was found while writing the public reference documentation: the
// tool said one thing and did another, and nothing in the suite noticed.
// ---------------------------------------------------------------------------

/// A theme directory holding `tokens.toml` and the empty `assets/` the loader
/// insists on.
fn theme_dir(name: &str, tokens: &str) -> Utf8PathBuf {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join(format!("iyo-cli-theme-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets").join("x.css"), "/* */").unwrap();
    std::fs::write(dir.join("tokens.toml"), tokens).unwrap();
    dir
}

/// A colour the gate cannot parse used to build a site and exit 0.
///
/// The gate recorded it as an **error**, counted it in `failed`, and the run
/// printed "theme contrast checked" having checked half the pairs, because the
/// scheme it belonged to was skipped and the build's refusal counted failing
/// *pairs*. A defect in the token file is true whatever scheme is published.
#[test]
fn an_unparseable_colour_token_refuses_the_build() {
    let theme = theme_dir(
        "unparseable",
        "schema = \"iyo.tokens/1\"\n\
         [meta]\nname = \"bad\"\nversion = \"1\"\n\
         [color.light]\ntext = \"not-a-colour\"\n",
    );
    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-unparseable-out");
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--theme",
            theme.as_str(),
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a theme whose colours cannot be read must not build. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !out.exists(),
        "the build refused and still wrote {out}; a refusal must leave nothing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("theme.token-syntax"),
        "the refusal must name the token at fault, not only count it: {stderr}"
    );
}

/// `--select term.no-labell` used to check nothing and exit 0.
///
/// In CI that is a permanently green job that verifies nothing, which is worse
/// than a red one.
#[test]
fn a_filter_matching_no_rule_is_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["check", "testdata/mini", "--select", "header.missingg"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "an unrecognised rule id is a usage error like any other"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("header.missing"),
        "the error should suggest the id that was meant: {stderr}"
    );

    // A family prefix is a legitimate filter and must still work.
    let ok = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["check", "testdata/mini", "--select", "term."])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");
    assert_eq!(ok.status.code(), Some(0), "a prefix is a valid filter");
}

/// `--ignore` can suppress error-level rules, turning an exit 1 into an exit 0.
/// That is what it is for; doing it silently is not.
#[test]
fn suppressed_findings_are_reported_rather_than_vanishing() {
    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "check",
            "testdata/mini",
            "--ignore",
            "term.",
            "--ignore",
            "release.",
            "--ignore",
            "header.",
            "--ignore",
            "site.",
            "--ignore",
            "text.",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("suppressed by --ignore"),
        "a run that reports nothing because everything was ignored must say so: {stderr}"
    );
}

/// `--dry-run`'s help says it reports what would be written. It reported the
/// digest and a file count of zero.
#[test]
fn a_dry_run_reports_the_files_it_would_write() {
    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-dryrun-out");
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
            "--dry-run",
            "--json",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(output.status.code(), Some(0));
    assert!(!out.exists(), "a dry run must create nothing");

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the dry run emits JSON");
    let files = report["files"].as_array().expect("a files array");
    assert!(
        files.len() > 50,
        "a dry run of the fixture should list what it would write, got {}",
        files.len()
    );
    assert!(
        files.iter().any(|f| f["path"] == "manifest.json"),
        "the listing should name real paths"
    );
}

/// A missing external program is an environment error. `typst` exited 4 and
/// `curl` exited 3 for the same class of failure.
#[test]
fn a_missing_curl_is_an_environment_error() {
    let empty = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-empty-path");
    std::fs::create_dir_all(&empty).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["conform", "https://example.org", "--timeout", "1"])
        .env("PATH", empty.as_str())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(
        output.status.code(),
        Some(4),
        "a missing external program is exit 4, as it is for typst. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A theme that only changes colours is a whole theme.
///
/// It used to be refused until an empty `assets/` sat beside its
/// `tokens.toml`, which is a rule about directory layout wearing the costume
/// of a rule about themes.
#[test]
fn a_theme_of_only_tokens_is_accepted() {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-tokens-only");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tokens.toml"),
        "schema = \"iyo.tokens/1\"\n\
         [meta]\nname = \"tokens-only\"\nversion = \"1\"\n\
         [color.light]\nlink = \"#0F609B\"\n",
    )
    .unwrap();

    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-tokens-only-out");
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--theme",
            dir.as_str(),
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a tokens-only theme must build. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let css = std::fs::read_to_string(out.join("assets").join("tokens.css"))
        .expect("the build wrote tokens.css");
    assert!(
        css.contains("#0F609B"),
        "the theme's token should reach the stylesheet"
    );

    // A directory with nothing to override is still refused.
    let empty = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-empty-theme");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--theme",
            empty.as_str(),
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");
    assert_eq!(
        refused.status.code(),
        Some(3),
        "a theme directory holding nothing is still an input error"
    );
}

/// A site served from a path audits against the server root, not the output
/// directory.
///
/// `404.html` is the one page whose links must be root-absolute, because it is
/// the one page whose own address is not known in advance. The auditor resolved
/// those against the output tree, which is the *site* root and sits below the
/// server root whenever a base path is in play, so every one of them was
/// reported broken: eight warnings on the fixture that no publisher could
/// clear, in the recipe the documentation recommends.
#[test]
fn a_site_served_from_a_path_has_no_unclearable_link_warnings() {
    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-cli-basepath-out");
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--out",
            out.as_str(),
            "--base-url",
            "https://example.org/",
            "--link-style",
            "file",
            "--base-path",
            "/preview/",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawning the built binary");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("a11y.broken-internal-link"),
        "a base path must not manufacture broken-link warnings: {stderr}"
    );

    // The links really are root-absolute and really do carry the base path:
    // the warnings are gone because they resolve, not because nothing looked.
    let page = std::fs::read_to_string(out.join("404.html")).expect("404.html was written");
    assert!(
        page.contains("href=\"/preview/"),
        "404.html should link through the base path"
    );
}
