//! Tests for the live probe.
//!
//! A probe that cannot fail is worse than none, so the test that matters is
//! not that a correct site passes but that a broken one is caught. Both run
//! against a server this test starts, so nothing here touches the network.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::{Ctx, manifest};
use iyo::site::Plan;
use iyo::{build, load, probe, profile, render, serve};
use std::time::Duration;

/// A deadline generous enough that none of these fixtures (a handful of
/// requests against a server on localhost) could ever reach it; these tests
/// are about what `probe::run` finds, not about the deadline itself.
const DEADLINE: Duration = Duration::from_secs(30);

fn curl_present() -> bool {
    std::process::Command::new("curl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build the fixture into a directory of its own and return it with its
/// manifest.
fn site(name: &str, base_url: &str) -> (Utf8PathBuf, manifest::Manifest) {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &root).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = base_url.to_owned();
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
        .join(format!("iyo-probe-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    output.write(&dir).unwrap();
    (dir, manifest)
}

#[test]
fn a_correct_site_passes_every_probe() {
    if !curl_present() {
        eprintln!("skipped: curl is not on PATH");
        return;
    }
    let (dir, manifest) = site("good", "https://example.org/");
    let (port, _thread) = serve::spawn(&dir, manifest.clone()).unwrap();
    let origin = format!("http://127.0.0.1:{port}");

    let report = probe::run(&manifest, &origin, None, 10, false, DEADLINE).unwrap();
    assert!(report.requests > 40, "only {} requests", report.requests);
    assert_eq!(
        report.errors,
        0,
        "a correct site reported errors: {:?}",
        &report.findings[..report.findings.len().min(3)]
    );
    assert_eq!(report.warnings, 0);
    // The probes a public conformance checker runs, over the same evidence.
    for name in ["URI1", "CN1", "RDF1", "VER2"] {
        assert_eq!(
            report.foops.get(name),
            Some(&Some(true)),
            "{name} did not pass"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_representation_is_caught() {
    if !curl_present() {
        eprintln!("skipped: curl is not on PATH");
        return;
    }
    let (dir, manifest) = site("broken", "https://example.org/");
    // The manifest still says this term has Turtle. Deleting the file is
    // exactly the defect the probe exists to find, and it is invisible from
    // the manifest alone.
    std::fs::remove_file(dir.join("vocab/Widget.ttl")).unwrap();
    // And a page that a consumer would follow an IRI to.
    std::fs::remove_file(dir.join("vocab/Thing.html")).unwrap();

    let (port, _thread) = serve::spawn(&dir, manifest.clone()).unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let report = probe::run(&manifest, &origin, None, 10, false, DEADLINE).unwrap();

    assert!(report.errors >= 2, "the probe missed a deleted file");
    let rules: Vec<&str> = report.findings.iter().map(|f| f.rule).collect();
    assert!(rules.contains(&"probe.no-representation"), "{rules:?}");
    assert!(rules.contains(&"probe.no-page"), "{rules:?}");
    // A failure has to reach the conformance summary, or a green score would
    // be reported beside red findings.
    assert_eq!(report.foops.get("URI1"), Some(&Some(false)));
    assert_eq!(report.foops.get("RDF1"), Some(&Some(false)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_origin_that_answers_nothing_is_an_error_not_a_pass() {
    if !curl_present() {
        eprintln!("skipped: curl is not on PATH");
        return;
    }
    let (dir, manifest) = site("dead", "https://example.org/");
    // Nothing is listening here. A probe that reported success against a host
    // that is down would be worse than useless.
    let report = probe::run(&manifest, "http://127.0.0.1:1", Some(2), 3, false, DEADLINE).unwrap();
    assert!(report.errors > 0);
    // One host that is down is one problem, not one per IRI: the per-request
    // findings collapse into the single fact they add up to. The count used
    // to scale with the ontology instead, 119 identical lines on this
    // fixture and 56,003 on a real site.
    assert_eq!(
        report.findings.len(),
        1,
        "a dead host should produce one finding, not one per request: {:?}",
        report.findings.iter().map(|f| f.rule).collect::<Vec<_>>()
    );
    assert_eq!(report.findings[0].rule, "probe.host-unreachable");
    assert!(
        report.findings[0]
            .message
            .contains(&report.made.to_string()),
        "the one finding says how many requests it stands for: {}",
        report.findings[0].message
    );
    // And every probe fails, not only the one whose rule list happened to
    // name `probe.unreachable`: nothing about this site was checked.
    for probe in ["URI1", "CN1", "RDF1"] {
        assert_eq!(
            report.foops.get(probe),
            Some(&Some(false)),
            "{probe} must not pass against a host that answered nothing: {:?}",
            report.foops_status
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sample_probes_fewer_terms_and_says_so() {
    let (dir, manifest) = site("sample", "https://example.org/");
    // No requests are made here: only the plan is compared.
    let all = probe::planned(&manifest, "http://example.invalid", None);
    let some = probe::planned(&manifest, "http://example.invalid", Some(1));
    assert!(some < all, "a sample did not shrink the request list");
    let _ = std::fs::remove_dir_all(&dir);
}
