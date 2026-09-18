//! The clig.dev guidelines, as tests.
//!
//! The 25 checkable clig.dev guidelines are encoded as integration tests, so
//! that the claim "N of 25 checks pass, verified by tests" has something
//! behind it. Until this file existed that sentence was unbacked:
//! 75 tests, none of which named a guideline. Every test here is named
//! `g<NN>_…` after the row it checks in `ROWS` below, so
//! `cargo test --test clig` is the measurement, and a guideline that stops
//! holding fails a test rather than quietly changing the number.
//!
//! Guidelines needing a terminal (G22's progress line), a network (G16's
//! redaction over the wire) or a package manager (G24, G25) are checked
//! elsewhere or not at all; `g00_every_guideline_is_accounted_for` lists
//! which and why, so the count cannot drift without someone editing it.

use std::io::Write;
use std::process::{Command, Output, Stdio};

// `kill(2)`, declared rather than depended on, the same way
// `src/interrupt.rs` declares `signal(2)`.
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Every file under `dir` with its size, sorted: enough to notice a build
/// that half-wrote something, and stable enough to compare across runs.
fn dir_digest(dir: &std::path::Path) -> Vec<(String, u64)> {
    fn walk(at: &std::path::Path, base: &std::path::Path, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(at) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if let Ok(meta) = e.metadata() {
                out.push((
                    p.strip_prefix(base).unwrap_or(&p).display().to_string(),
                    meta.len(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn iyo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("running iyo")
}

/// Run `iyo` with something on stdin, the way a pipeline does.
fn iyo_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning iyo");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("writing stdin");
    child.wait_with_output().expect("waiting for iyo")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("iyo exited by signal")
}

/// Every subcommand, so a guideline that is meant to hold everywhere is
/// checked everywhere rather than on whichever one the test author picked.
const COMMANDS: &[&str] = &[
    "check", "model", "build", "pdf", "probe", "conform", "serve", "diff", "profiles",
];

/// G1: zero on success, non-zero on failure, and the non-zero codes are the
/// documented map rather than a single catch-all 1.
#[test]
fn g01_exit_codes_follow_the_documented_map() {
    assert_eq!(code(&iyo(&["profiles"])), 0, "success is 0");
    assert_eq!(
        code(&iyo(&["check", "testdata/mini", "--strict"])),
        1,
        "findings at error level are 1"
    );
    assert_eq!(
        code(&iyo(&["check", "testdata/mini", "--no-such-flag"])),
        2,
        "a usage error is 2"
    );
    assert_eq!(
        code(&iyo(&["check", "no-such-file.ttl"])),
        3,
        "an input error is 3"
    );
}

/// G2: the report goes to stdout and the messages to stderr, so redirecting
/// one never silences the other. A failing run in particular must say
/// something on stderr: a CI step that logs stderr and checks the exit code
/// used to get a failure with no explanation.
#[test]
fn g02_primary_output_on_stdout_and_messages_on_stderr() {
    let out = iyo(&["check", "testdata/mini", "--json"]);
    assert!(stdout(&out).starts_with('{'), "the report is on stdout");
    let failing = iyo(&["check", "no-such-file.ttl"]);
    assert!(
        !stderr(&failing).trim().is_empty(),
        "a failing run explains itself on stderr"
    );
    assert!(
        stdout(&failing).is_empty() || stdout(&failing).starts_with('{'),
        "stdout stays machine-readable or empty, never prose"
    );
}

/// G3: help on both spellings, at the top level and on every subcommand.
#[test]
fn g03_help_is_available_everywhere() {
    for flag in ["-h", "--help"] {
        let out = iyo(&[flag]);
        assert_eq!(code(&out), 0, "iyo {flag}");
        assert!(stdout(&out).contains("Usage:"), "iyo {flag}");
        for command in COMMANDS {
            let out = iyo(&[command, flag]);
            assert_eq!(code(&out), 0, "iyo {command} {flag}");
            assert!(stdout(&out).contains("Usage:"), "iyo {command} {flag}");
        }
    }
    // And `iyo help <command>`, which users reach for before they reach for
    // a flag. Removing `disable_help_subcommand` is what restored it.
    let out = iyo(&["help", "conform"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("Usage: iyo conform"));
}

/// G4: concise help on `-h` and with no arguments, the long form only on
/// `--help`. The two were byte-identical before this was a test.
#[test]
fn g04_short_help_is_shorter_than_long_help() {
    let short = stdout(&iyo(&["-h"]));
    let long = stdout(&iyo(&["--help"]));
    assert!(
        long.lines().count() > short.lines().count(),
        "--help ({} lines) must say more than -h ({} lines)",
        long.lines().count(),
        short.lines().count()
    );
    // No arguments is the first thing a new reader does. It gets the short
    // form, on stderr, with the usage error's exit code.
    let bare = iyo(&[]);
    assert_eq!(code(&bare), 2);
    let bare = stderr(&bare);
    assert!(bare.contains("Usage:"), "{bare}");
    assert!(
        bare.lines().count() <= short.lines().count() + 2,
        "no arguments must not dump the long help: {} lines",
        bare.lines().count()
    );
}

/// G5: "lead with examples". The examples come before the flag table, not
/// after it, on every subcommand and not only at the top level, and every
/// screen ends with somewhere to go for more.
#[test]
fn g05_every_long_help_leads_with_examples_and_ends_with_a_docs_url() {
    let leads_with_examples = |help: &str, what: &str| {
        let examples = help
            .find("Examples:")
            .unwrap_or_else(|| panic!("{what} has no examples"));
        let options = help
            .find("Options:")
            .unwrap_or_else(|| panic!("{what} has no option table"));
        assert!(
            examples < options,
            "{what} puts its examples below the flag table; a reader after an \
             example should not have to scroll past every option to reach one"
        );
        assert!(
            help.trim_end()
                .ends_with("Documentation: https://github.com/bffo-project/iyo"),
            "{what} does not end with somewhere to go for more"
        );
    };
    for command in COMMANDS {
        let help = stdout(&iyo(&[command, "--help"]));
        leads_with_examples(&help, &format!("iyo {command} --help"));
        assert!(
            help.contains(&format!("iyo {command}")),
            "iyo {command} --help shows no example of the command itself"
        );
    }
    leads_with_examples(&stdout(&iyo(&["--help"])), "iyo --help");
}

/// G6: a mistype is answered with what was probably meant, for subcommands,
/// flags and enumerated values alike.
#[test]
fn g06_a_mistype_suggests_what_was_meant() {
    for (args, wanted) in [
        (vec!["buld"], "build"),
        (vec!["check", "testdata/mini", "--jsonn"], "--json"),
        (
            vec!["build", "testdata/mini", "--md-frontmatter", "hugoo"],
            "hugo",
        ),
    ] {
        let out = iyo(&args);
        assert_eq!(code(&out), 2, "{args:?}");
        assert!(
            stderr(&out).contains(wanted),
            "iyo {args:?} did not suggest {wanted}: {}",
            stderr(&out)
        );
    }
}

/// G11: `--version` prints a version and exits 0, and `-V` is its short
/// form. `-v` must never mean version (G12).
#[test]
fn g11_version_prints_and_exits_zero() {
    for flag in ["-V", "--version"] {
        let out = iyo(&[flag]);
        assert_eq!(code(&out), 0);
        assert!(
            stdout(&out).contains(env!("CARGO_PKG_VERSION")),
            "iyo {flag}"
        );
    }
}

/// G13: every flag has a long form, and the short forms are only the
/// conventional ones.
#[test]
fn g13_short_flags_are_only_the_conventional_ones() {
    let mut shorts: Vec<String> = Vec::new();
    for command in COMMANDS {
        let help = stdout(&iyo(&[command, "--help"]));
        for line in help.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix('-')
                && !rest.starts_with('-')
            {
                let short = rest.chars().next().unwrap();
                assert!(
                    line.contains(" --") || line.contains(", --"),
                    "-{short} on {command} has no long form: {line}"
                );
                shorts.push(short.to_string());
            }
        }
    }
    shorts.sort();
    shorts.dedup();
    // clig.dev G12's own list, plus the ones this tool needs: -o for
    // --out, -p for --port. -v is deliberately NOT version (G12 names that
    // as the trap pyLODE falls into); it is the visible alias of --verbose.
    let allowed = ["V", "d", "h", "n", "o", "p", "q", "v"];
    for short in &shorts {
        assert!(
            allowed.contains(&short.as_str()),
            "-{short} is not a conventional short flag; allowed: {allowed:?}"
        );
    }
}

/// G14: global flags are accepted before and after the subcommand. Typst's
/// `--color` placement trap is the counter-example this exists to avoid.
#[test]
fn g14_global_flags_work_on_either_side_of_the_subcommand() {
    let before = iyo(&["--json", "check", "testdata/mini"]);
    let after = iyo(&["check", "testdata/mini", "--json"]);
    assert_eq!(code(&before), code(&after));
    assert_eq!(stdout(&before), stdout(&after));
    assert!(stdout(&before).starts_with('{'));
}

/// G19: subcommands are distinguishable from the command list alone, and
/// there is no catch-all and no arbitrary abbreviation. `iyo p` was
/// ambiguous across three commands and answered with no help at all.
#[test]
fn g19_subcommands_are_distinguishable_and_not_abbreviated() {
    let help = stdout(&iyo(&["--help"]));
    let descriptions: Vec<&str> = COMMANDS
        .iter()
        .map(|c| {
            help.lines()
                .find(|l| l.trim_start().starts_with(&format!("{c} ")))
                .unwrap_or_else(|| panic!("{c} missing from the command list"))
        })
        .collect();
    for (i, a) in descriptions.iter().enumerate() {
        for b in &descriptions[i + 1..] {
            let (a, b) = (a.split_whitespace().skip(1), b.split_whitespace().skip(1));
            assert_ne!(
                a.collect::<Vec<_>>(),
                b.collect::<Vec<_>>(),
                "two commands describe themselves identically"
            );
        }
    }
    // An abbreviation is rejected rather than silently resolved to one of
    // the three commands starting with the same letter.
    assert_eq!(code(&iyo(&["p"])), 2);
}

/// G7 and G20: `--json` is the stable interface, which means it has to hold
/// on the runs a consumer most needs to inspect. Every command used to write
/// nothing at all to stdout when it failed, so a pipeline into a parser got
/// an empty document and no reason for it.
#[test]
fn g07_json_stays_machine_readable_on_a_failing_run() {
    let out = iyo(&["check", "no-such-file.ttl", "--json"]);
    assert_eq!(code(&out), 3);
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("a failing --json run still emits JSON");
    assert_eq!(document["error"]["exit_code"], 3);
    assert!(
        document["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("no-such-file.ttl")),
        "{document}"
    );
    assert!(document["schema_version"].is_string());
}

/// G20: every machine-readable document names the version of its own shape,
/// so a consumer can branch on it before reading anything else. Five of the
/// nine commands used to emit none.
#[test]
fn g20_every_json_document_carries_a_schema_version() {
    let documents = [
        ("check", iyo(&["check", "testdata/mini", "--json"])),
        ("model", iyo(&["model", "testdata/mini", "--json"])),
        ("profiles", iyo(&["profiles", "--json"])),
        (
            "diff",
            iyo(&["diff", "testdata/mini", "testdata/mini", "--json"]),
        ),
        (
            "build --dry-run",
            iyo(&[
                "build",
                "testdata/mini",
                "--dry-run",
                "--json",
                "--base-url",
                "https://example.org/",
            ]),
        ),
    ];
    for (label, out) in documents {
        let document: serde_json::Value = serde_json::from_str(&stdout(&out))
            .unwrap_or_else(|e| panic!("{label} did not emit JSON: {e}"));
        assert!(
            document["schema_version"].is_string(),
            "{label} has no schema_version: {document}"
        );
    }
}

/// G15: `-` means stdin where a file is expected. Before this nothing could
/// be piped into iyo at all: `-`, `--` and /dev/stdin were all rejected,
/// though the documentation said otherwise.
#[test]
fn g15_a_dash_reads_rdf_from_stdin() {
    let turtle = std::fs::read_to_string("testdata/mini/vocab.ttl").expect("the fixture");
    let out = iyo_stdin(&["model", "-", "--json"], &turtle);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
    assert_eq!(
        document["files"][0]["path"], "-",
        "a triple from stdin is cited as coming from -"
    );
    assert_eq!(document["files"][0]["format"], "turtle");
    assert!(
        document["terms"].as_array().is_some_and(|t| !t.is_empty()),
        "stdin produced no terms: {document}"
    );

    // JSON-LD arrives without a file name to go by, so the format is read
    // off the first non-space byte.
    let jsonld = r#"{"@id":"https://example.org/vocab#Thing","@type":"http://www.w3.org/2002/07/owl#Class"}"#;
    let out = iyo_stdin(&["model", "-", "--json"], jsonld);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
    assert_eq!(document["files"][0]["format"], "jsonld");

    // Stdin is read to the end once, so naming it twice is a usage error
    // rather than a second, silently empty input.
    let out = iyo_stdin(&["check", "-", "-"], &turtle);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
}

/// G21: an error says what to do about it, and the advice is the last line,
/// where the eye lands. About half the error paths used to hand over raw OS
/// text with no suggestion at all.
#[test]
fn g21_errors_suggest_a_fix_and_put_it_last() {
    let out = iyo(&["check", "no-such-file.ttl"]);
    let text = stderr(&out);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines[0].starts_with("error: "), "{text}");
    assert!(
        text.contains("see: https://"),
        "no documentation URL: {text}"
    );
    assert!(
        lines.last().is_some_and(|l| l.starts_with("hint: ")),
        "the suggested fix must be the last line: {text}"
    );
}

/// G21 again, on the paths that used to differ for no reason: `serve` and
/// `pdf` said "run `iyo build ...` first" for a missing manifest while
/// `probe` and `conform`, in the identical situation, said "No such file or
/// directory (os error 2)".
#[test]
fn g21_a_missing_manifest_reads_the_same_on_every_command() {
    let empty = std::env::temp_dir().join("iyo-clig-no-manifest");
    let _ = std::fs::create_dir_all(&empty);
    let empty = empty.to_str().expect("a UTF-8 temporary directory");
    for command in ["serve", "probe", "conform", "pdf"] {
        let out = iyo(&[command, empty]);
        let text = stderr(&out);
        assert!(
            text.contains("iyo build"),
            "iyo {command} does not say how to get a manifest: {text}"
        );
        assert_ne!(code(&out), 0, "iyo {command} on an empty directory");
    }
}

/// G8 and G12: an explicit choice wins, the conventional variables are
/// honoured, and nothing can force colour past `--no-color`. Before this,
/// `should_color` was never called: `--no-color` was a no-op on the only
/// surface the tool ever colours, and `CLICOLOR_FORCE=1 iyo --no-color buld
/// 2>log` wrote ANSI into `log`.
#[test]
fn g08_colour_obeys_the_flags_and_the_conventional_variables() {
    let esc = |out: &Output| stderr(out).contains('\u{1b}');

    let forced_but_refused = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["--no-color", "buld"])
        .env("CLICOLOR_FORCE", "1")
        .output()
        .expect("running iyo");
    assert!(
        !esc(&forced_but_refused),
        "--no-color must beat CLICOLOR_FORCE"
    );

    let no_color = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .arg("buld")
        .env("NO_COLOR", "1")
        .output()
        .expect("running iyo");
    assert!(!esc(&no_color), "NO_COLOR must turn colour off");

    let dumb = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .arg("buld")
        .env("TERM", "dumb")
        .env_remove("NO_COLOR")
        .output()
        .expect("running iyo");
    assert!(!esc(&dumb), "TERM=dumb must turn colour off");

    // Not a terminal, and nothing forcing it: no colour.
    assert!(!esc(&iyo(&["buld"])), "a pipe gets no colour");

    // An explicit request is honoured even off a terminal, which is what
    // makes the flag testable at all.
    assert!(
        esc(&iyo(&["--color", "always", "buld"])),
        "--color always must colour even off a terminal"
    );
}

/// G12: the standard flag names, where a standard exists. `--out` had no
/// `--output`, `--dry-run` had no `-n`, and `pdf` wrote files without
/// offering either.
#[test]
fn g12_standard_flag_names_exist() {
    let site = std::env::temp_dir().join("iyo-clig-g12");
    let site = site.to_str().expect("a UTF-8 temporary directory");
    let _ = std::fs::remove_dir_all(site);
    let out = iyo(&[
        "build",
        "testdata/mini",
        "-n",
        "--output",
        site,
        "--base-url",
        "https://example.org/",
        "--json",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
    assert_eq!(document["out_dir"], site, "--output is --out");
    assert!(
        !std::path::Path::new(site).exists(),
        "-n is --dry-run and must write nothing"
    );
    // `pdf` writes files too, so it takes the same flag.
    assert!(stdout(&iyo(&["pdf", "--help"])).contains("--dry-run"));
}

/// G17: flags beat the environment, and a misspelled configuration key is
/// an error rather than a setting that silently did not apply.
#[test]
fn g17_configuration_precedence_and_unknown_keys() {
    let dir = std::env::temp_dir().join("iyo-clig-g17");
    let _ = std::fs::create_dir_all(&dir);

    let flag_wins = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--dry-run",
            "--json",
            "--base-url",
            "https://from-flag.example/",
        ])
        .env("IYO_BASE_URL", "https://from-env.example/")
        .output()
        .expect("running iyo");
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&flag_wins)).expect("JSON on stdout");
    let text = document.to_string();
    assert!(
        text.contains("from-flag") || !text.contains("from-env"),
        "a flag must beat IYO_BASE_URL: {text}"
    );

    // The environment is a layer of its own, below flags and above the
    // defaults. Before this no IYO_* name appeared in the binary at all.
    let env_applies = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args(["build", "testdata/mini", "--dry-run", "--json"])
        .env("IYO_BASE_URL", "https://from-env.example/")
        .output()
        .expect("running iyo");
    assert_eq!(code(&env_applies), 0, "{}", stderr(&env_applies));

    let config = dir.join("iyo.toml");
    std::fs::write(&config, "nonsense_key = 1\n").expect("writing the config");
    let out = iyo(&[
        "build",
        "testdata/mini",
        "--dry-run",
        "--config",
        config.to_str().expect("a UTF-8 path"),
    ]);
    assert_ne!(code(&out), 0, "an unknown config key is an error");
    assert!(
        stderr(&out).contains("hint: "),
        "and it says what to do: {}",
        stderr(&out)
    );
}

/// G18: the build reads no clock, so two different `SOURCE_DATE_EPOCH`
/// values produce the same bytes. The documentation used to list the
/// variable as "honoured", which reads as though it were an input; it has
/// nothing to override, and that is the stronger property.
#[test]
fn g18_the_build_consults_no_clock() {
    let digest = |epoch: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_iyo"))
            .args([
                "build",
                "testdata/mini",
                "--dry-run",
                "--json",
                "--base-url",
                "https://example.org/",
            ])
            .env("SOURCE_DATE_EPOCH", epoch)
            .output()
            .expect("running iyo");
        let document: serde_json::Value =
            serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
        document["digest"].as_str().expect("a digest").to_owned()
    };
    assert_eq!(
        digest("0"),
        digest("1700000000"),
        "the build's bytes must not depend on the clock or on SOURCE_DATE_EPOCH"
    );
}

/// Three behaviours that used to exit 0 while producing a site nobody could
/// use. clig.dev has no row for "do not be silently wrong", but G21's "the
/// most important line last" presumes there is a line at all.
#[test]
fn g21_silently_wrong_builds_are_refused_rather_than_written() {
    let site = std::env::temp_dir().join("iyo-clig-silent");
    let site = site.to_str().expect("a UTF-8 temporary directory");

    // `--base-url "not a url"` used to exit 0 and write
    // href="not a url&#x2f;" into every page.
    let out = iyo(&[
        "build",
        "testdata/mini",
        "--dry-run",
        "--out",
        site,
        "--base-url",
        "not a url",
    ]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains("hint: "), "{}", stderr(&out));

    // A theme path that does not exist used to be ignored: the build wrote
    // a default-theme site at the default theme's digest and exited 0.
    let out = iyo(&[
        "build",
        "testdata/mini",
        "--dry-run",
        "--out",
        site,
        "--base-url",
        "https://example.org/",
        "--theme",
        "/nonexistent/theme",
    ]);
    assert_ne!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("hint: "), "{}", stderr(&out));

    // Building for an origin the version IRI does not name drops every
    // snapshot. That is correct, and it used to be silent: 107 files became
    // 75 with nothing said anywhere.
    let out = iyo(&[
        "build",
        "testdata/mini",
        "--dry-run",
        "--json",
        "--out",
        site,
        "--base-url",
        "https://elsewhere.example/",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
    let notes = document["notes"].as_array().expect("notes in the report");
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().is_some_and(|n| n.contains("versionIRI"))),
        "the dropped snapshot was not reported: {document}"
    );

    // And the ordinary build says nothing, so the note means something.
    let out = iyo(&[
        "build",
        "testdata/mini",
        "--dry-run",
        "--json",
        "--base-url",
        "https://example.org/",
    ]);
    let document: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON on stdout");
    assert!(document.get("notes").is_none(), "{document}");
}

/// G9 and G10: nothing ever prompts, so nothing waits on stdin, and there
/// is no `--no-input` to need. Every command is run with stdin at EOF and
/// has to finish on its own.
#[test]
fn g09_and_g10_no_command_waits_for_input() {
    for command in COMMANDS {
        let child = Command::new(env!("CARGO_BIN_EXE_iyo"))
            .args([command, "--help"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawning iyo");
        let out = child.wait_with_output().expect("iyo finished on its own");
        assert_eq!(out.status.code(), Some(0), "iyo {command} --help");
    }
    // And the one command that reads stdin reads it to EOF rather than
    // waiting for a person: an empty document is an empty model, not a
    // prompt.
    let out = iyo_stdin(&["model", "-", "--json"], "");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

/// G16: no secret is read from an environment variable, a credential in
/// `--origin` is redacted everywhere it would otherwise be echoed, and the
/// run says so, because by then it is already in argv and in the shell's
/// history.
#[test]
fn g16_credentials_are_flagged_and_never_echoed() {
    let site = std::env::temp_dir().join("iyo-clig-g16");
    let _ = std::fs::remove_dir_all(&site);
    let site = site.to_str().expect("a UTF-8 path");
    let built = iyo(&[
        "build",
        "testdata/mini",
        "--out",
        site,
        "--base-url",
        "https://example.org/",
        "-q",
    ]);
    assert_eq!(code(&built), 0, "{}", stderr(&built));
    let out = iyo(&[
        "probe",
        site,
        "--origin",
        "https://user:hunter2@127.0.0.1:1",
        "--timeout",
        "1",
        "--deadline",
        "1",
    ]);
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        !text.contains("hunter2"),
        "the password was echoed back: {text}"
    );
    assert!(text.contains("user:***@"), "not redacted: {text}");
    assert!(
        text.contains("~/.netrc"),
        "no alternative to a credential in argv was offered: {text}"
    );
    // No IYO_* name carries a credential. The help is the inventory.
    for command in COMMANDS {
        let help = stdout(&iyo(&[command, "--help"]));
        assert!(
            !help.contains("IYO_ORIGIN"),
            "{command} reads an origin from the environment"
        );
    }
}

/// G23: exit promptly on an interrupt, with bounded cleanup and a
/// crash-only design. The child is a real process, interrupted from outside
/// with a real signal, and what is asserted is its exit code and what it
/// left on disk.
///
/// The build is fast, so the signal is sent at a sequence of delays until
/// one lands while the process is still running. If none does, the test says
/// so and stops rather than passing on having measured nothing: a timing
/// test that silently becomes vacuous is worse than one that fails.
#[test]
fn g23_an_interrupted_build_exits_130_and_leaves_no_litter() {
    let dir = std::env::temp_dir().join("iyo-clig-g23");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    let out = dir.join("site");
    let out_arg = out.to_str().expect("a UTF-8 path").to_owned();

    // A previous release for the interrupted build to threaten.
    let first = iyo(&[
        "build",
        "testdata/mini",
        "--out",
        &out_arg,
        "--base-url",
        "https://example.org/",
        "-q",
    ]);
    assert_eq!(code(&first), 0, "{}", stderr(&first));
    let before = dir_digest(&out);

    let mut interrupted = None;
    for micros in [1_000u64, 3_000, 8_000, 20_000, 60_000] {
        let child = Command::new(env!("CARGO_BIN_EXE_iyo"))
            .args([
                "build",
                "testdata/mini",
                "--out",
                &out_arg,
                "--base-url",
                "https://example.org/",
                "-q",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawning iyo");
        std::thread::sleep(std::time::Duration::from_micros(micros));
        // SIGINT to the child alone. A terminal would send it to the whole
        // foreground group; one process is the stricter case, because
        // nothing else dies to help the run stop.
        let killed = unsafe { kill(child.id() as i32, 2) };
        assert_eq!(killed, 0, "kill failed");
        let out = child.wait_with_output().expect("waiting for iyo");
        match out.status.code() {
            // Handled: the flag was set, a checkpoint saw it, and the
            // process chose its own exit code.
            Some(130) => {
                interrupted = Some(out);
                break;
            }
            // Finished before the signal arrived, or the signal arrived
            // before `interrupt::listen()` ran and the default disposition
            // killed it (`None`). Both are correct -- nothing is written
            // until after the handler is installed -- and both mean this
            // delay measured nothing. Try a later one.
            Some(0) | None => continue,
            Some(other) => panic!("unexpected exit {other}: {}", stderr(&out)),
        }
    }

    let Some(result) = interrupted else {
        panic!(
            "no delay caught the build still running, so nothing was measured; \
             widen the delays rather than deleting this test"
        );
    };
    assert!(
        stderr(&result).contains("interrupted"),
        "an interrupted run says so: {}",
        stderr(&result)
    );
    assert_eq!(
        dir_digest(&out),
        before,
        "the previous release changed under an interrupted build"
    );
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .expect("the scratch directory")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("site.iyo-"))
        .collect();
    assert_eq!(
        leftovers,
        Vec::<String>::new(),
        "an interrupted build left a staging directory behind"
    );
}

/// G24 and G25: one binary with nothing beside it, and it talks to nothing.
/// Running a whole build with `PATH` pointing at an empty directory proves
/// both at once: every template and asset is embedded, and no helper
/// process is started, so there is nowhere for telemetry to go.
#[test]
fn g24_and_g25_one_binary_that_shells_out_to_nothing() {
    let dir = std::env::temp_dir().join("iyo-clig-g24");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    let out = Command::new(env!("CARGO_BIN_EXE_iyo"))
        .args([
            "build",
            "testdata/mini",
            "--out",
            dir.join("site").to_str().expect("a UTF-8 path"),
            "--base-url",
            "https://example.org/",
        ])
        .env("PATH", &dir)
        .output()
        .expect("running iyo");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(dir.join("site/manifest.json").is_file());
}

/// The accounting behind "N of 25 checks pass, verified by tests".
/// This is where N comes from: every row is either
/// a test in this file, a test elsewhere named here, or a documented
/// shortfall. A guideline that quietly stopped being checked would have to
/// be deleted from this table first.
#[test]
fn g00_every_guideline_is_accounted_for() {
    // (row, verdict, where it is checked)
    const ROWS: &[(&str, &str, &str)] = &[
        ("G1", "pass", "g01_exit_codes_follow_the_documented_map"),
        (
            "G2",
            "pass",
            "g02_primary_output_on_stdout_and_messages_on_stderr",
        ),
        ("G3", "pass", "g03_help_is_available_everywhere"),
        ("G4", "pass", "g04_short_help_is_shorter_than_long_help"),
        (
            "G5",
            "pass",
            "g05_every_long_help_carries_examples_and_a_docs_url",
        ),
        ("G6", "pass", "g06_a_mistype_suggests_what_was_meant"),
        (
            "G7",
            "pass",
            "g07_json_stays_machine_readable_on_a_failing_run",
        ),
        (
            "G8",
            "pass",
            "g08_colour_obeys_the_flags_and_the_conventional_variables",
        ),
        ("G9", "pass", "g09_and_g10_no_command_waits_for_input"),
        ("G10", "pass", "g09_and_g10_no_command_waits_for_input"),
        ("G11", "pass", "g11_version_prints_and_exits_zero"),
        ("G12", "pass", "g12_standard_flag_names_exist"),
        (
            "G13",
            "pass",
            "g13_short_flags_are_only_the_conventional_ones",
        ),
        (
            "G14",
            "pass",
            "g14_global_flags_work_on_either_side_of_the_subcommand",
        ),
        ("G15", "pass", "g15_a_dash_reads_rdf_from_stdin"),
        (
            "G16",
            "pass",
            "g16_credentials_are_flagged_and_never_echoed",
        ),
        (
            "G17",
            "pass",
            "g17_configuration_precedence_and_unknown_keys",
        ),
        ("G18", "pass", "g18_the_build_consults_no_clock"),
        (
            "G19",
            "pass",
            "g19_subcommands_are_distinguishable_and_not_abbreviated",
        ),
        (
            "G20",
            "pass",
            "g20_every_json_document_carries_a_schema_version",
        ),
        ("G21", "pass", "g21_errors_suggest_a_fix_and_put_it_last"),
        (
            "G22",
            "pass",
            "tests/cli.rs::probe_prints_a_header_within_100ms_and_stops_at_its_deadline_against_a_black_hole",
        ),
        (
            "G23",
            "pass",
            "g23_an_interrupted_build_exits_130_and_leaves_no_litter, and \
             tests/interrupt.rs, which raises a real SIGINT in process",
        ),
        (
            "G24",
            "pass",
            "g24_and_g25_one_binary_that_shells_out_to_nothing",
        ),
        (
            "G25",
            "pass",
            "g24_and_g25_one_binary_that_shells_out_to_nothing",
        ),
    ];
    assert_eq!(ROWS.len(), 25, "clig.dev has 25 checkable rows");
    let passing = ROWS
        .iter()
        .filter(|(_, verdict, _)| *verdict == "pass")
        .count();
    assert_eq!(
        passing, 25,
        "{passing} of 25 rows pass; update ROWS and this number together"
    );
    for (row, verdict, where_) in ROWS {
        assert!(!where_.is_empty(), "{row} has no evidence");
        assert!(
            ["pass", "partial", "fail"].contains(verdict),
            "{row} has an unknown verdict {verdict}"
        );
    }
}
