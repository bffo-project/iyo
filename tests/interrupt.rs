//! What a real SIGINT does to a real build, in a test binary of its own.
//!
//! The interrupt flag is process-global, so setting it here would change what
//! every other test in the same binary sees. Cargo compiles each file under
//! `tests/` into its own executable, which is why this file exists rather
//! than a few more cases in `tests/cli.rs`: the signal is raised for real,
//! caught by the real handler, and nothing else runs beside it.
//!
//! `tests/clig.rs::g23_*` covers the other half, a child process interrupted
//! from outside with its exit code observed.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, interrupt, load, profile, render};

// `raise(3)`, declared the same way `src/interrupt.rs` declares `signal(2)`
// and for the same reason: libc is linked already and this is one symbol.
unsafe extern "C" {
    fn raise(sig: i32) -> i32;
}

const SIGINT: i32 = 2;

fn built_output() -> render::Output {
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
    render::render(&ctx).unwrap()
}

fn tree(dir: &Utf8PathBuf) -> Vec<(String, u64)> {
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if let Ok(meta) = e.metadata() {
                let rel = p.strip_prefix(base).unwrap_or(&p).display().to_string();
                out.push((rel, meta.len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir.as_std_path(), dir.as_std_path(), &mut out);
    out.sort();
    out
}

fn staging_beside(out: &Utf8PathBuf) -> Vec<String> {
    let parent = out.parent().expect("a parent");
    let prefix = format!("{}.iyo-", out.file_name().expect("a name"));
    std::fs::read_dir(parent.as_std_path())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix))
        .collect()
}

/// The whole of clig.dev G23, as one test.
///
/// One test and not three, because the flag is process-global and Rust runs
/// tests in a binary on parallel threads: a second test raising SIGINT would
/// set the flag under this one's baseline build, which is exactly what
/// happened when this file was first written. The alternative, forcing
/// `--test-threads=1`, would be a property of how the suite is invoked
/// rather than of the test, and would break silently the first time someone
/// ran the file on its own.
///
/// What it pins: a build interrupted part-way leaves the previous release
/// exactly as it was, removes its own staging directory rather than leaving
/// it for the next run to find, reports the code a shell would have
/// synthesised anyway, and keeps refusing at every later checkpoint rather
/// than letting the next step of a script run half-way.
///
/// Before the handler existed, the process died where it stood: `--out` was
/// safe because the swap is a rename, but `<out>.iyo-partial-<pid>` stayed
/// behind and 130 came from the shell rather than from any decision here.
#[test]
fn an_interrupted_build_leaves_the_previous_release_and_no_litter() {
    let out = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-interrupt-build");
    let _ = std::fs::remove_dir_all(&out);
    for name in staging_beside(&out) {
        let _ = std::fs::remove_dir_all(out.parent().unwrap().join(name));
    }

    // A previous release, written normally, is what the interrupt must not
    // damage. This has to happen before the flag is set.
    built_output().write(&out).expect("the first build");
    let before = tree(&out);
    assert!(!before.is_empty(), "the fixture wrote nothing");

    assert!(interrupt::listen(), "the handler did not install");
    assert!(!interrupt::pending(), "something set the flag already");
    assert_eq!(unsafe { raise(SIGINT) }, 0, "raise failed");
    assert!(
        interrupt::pending(),
        "a real SIGINT did not reach the handler"
    );

    let err = built_output()
        .write(&out)
        .expect_err("an interrupted write must fail rather than half-finish");
    let (code, hint) = iyo::Failure::of(&err);
    assert_eq!(code, 130, "an interrupt is exit 130: {err:#}");
    assert!(hint.is_some_and(|h| h.contains("half written")), "{err:#}");

    assert_eq!(
        tree(&out),
        before,
        "the previous release changed under an interrupted build"
    );
    assert_eq!(
        staging_beside(&out),
        Vec::<String>::new(),
        "an interrupted build left its staging directory behind"
    );

    // The flag survives, so a later step stops too rather than running
    // half-way after the operator has already asked for the run to end.
    let err = interrupt::check().expect_err("check must fail once interrupted");
    assert_eq!(iyo::Failure::of(&err).0, 130);
    assert_eq!(format!("{err}"), "interrupted");

    let _ = std::fs::remove_dir_all(&out);
}
