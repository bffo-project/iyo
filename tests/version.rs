//! Tests for versioned snapshots.
//!
//! A snapshot exists so that a citation keeps working: `owl:versionIRI` must
//! resolve to the release it names, not to whatever the vocabulary has since
//! become. The strongest thing to assert is therefore not that a snapshot
//! exists but that it is a faithful copy, because a snapshot that quietly
//! differs from the release it claims to be is worse than none.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, load, profile, render};
use std::collections::{BTreeMap, BTreeSet};

const SNAPSHOT: &str = "https://example.org/vocab/0.1.0/";
const LATEST: &str = "https://example.org/vocab/";

fn built(with: impl Fn(&mut Config)) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    with(&mut config);
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    render::render(&ctx).unwrap().into_files()
}

fn under(files: &BTreeMap<String, String>, prefix: &str) -> BTreeSet<String> {
    files
        .keys()
        .filter(|p| p.starts_with(prefix))
        .map(|p| p[prefix.len()..].to_owned())
        .collect()
}

#[test]
fn the_version_iri_resolves_to_the_snapshot() {
    let files = built(|_| {});
    // The document of the snapshot is what `owl:versionIRI` names.
    assert!(files.contains_key("vocab/0.1.0/index.html"));
    assert!(files.contains_key("vocab/0.1.0/index.md"));
    assert!(files.contains_key("vocab/0.1.0/ex.ttl"));

    let page = &files["vocab/0.1.0/index.html"];
    assert!(
        page.contains(&format!("canonical\" href=\"{SNAPSHOT}\"")),
        "an archived page must be canonical for itself, not for latest"
    );

    let versions: serde_json::Value = serde_json::from_str(&files["versions.json"]).unwrap();
    let entry = versions["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["iri"] == LATEST)
        .expect("the namespace is listed");
    assert_eq!(entry["versions"][0]["url"], SNAPSHOT);
    assert_eq!(entry["versions"][0]["version_iri_resolves"], true);
    assert_eq!(entry["versions"][0]["source"], "version-info");
}

#[test]
fn a_snapshot_holds_the_same_files_as_the_release_it_copies() {
    let files = built(|_| {});
    let snapshot = under(&files, "vocab/0.1.0/");
    let latest: BTreeSet<String> = under(&files, "vocab/")
        .into_iter()
        // Not the snapshot's own subtree, not the nested shapes namespace,
        // which is a namespace of its own and archives separately, and not
        // the files only latest has.
        .filter(|p| !p.starts_with("0.1.0/") && !p.starts_with("shapes/") && p != "versions.ttl")
        .collect();

    assert!(!snapshot.is_empty(), "no snapshot was written");
    assert_eq!(
        snapshot, latest,
        "the snapshot and the release it copies hold different files"
    );
    // The version links belong to latest alone: a release does not list
    // releases that came after it.
    assert!(files.contains_key("vocab/versions.ttl"));
    assert!(!files.contains_key("vocab/0.1.0/versions.ttl"));
}

#[test]
fn a_snapshot_is_the_release_with_its_urls_moved() {
    let files = built(|_| {});
    let mut differing = Vec::new();
    for rel in under(&files, "vocab/0.1.0/") {
        // The document page is the one file that legitimately differs: it
        // lists the releases, and a release does not list itself.
        if rel.starts_with("index.") || rel == "llms.txt" {
            continue;
        }
        let snapshot = &files[&format!("vocab/0.1.0/{rel}")];
        let latest = &files[&format!("vocab/{rel}")];
        // Move the snapshot's URLs back, and its stylesheet one level up,
        // and nothing else should be left over.
        // Navigation and assets are relative, so a snapshot page differs from
        // the latest one twice over: its own targets carry the version
        // segment, and everything it points at is one directory further away
        // because the page itself is one directory deeper. Undo both, and
        // nothing else should be left over.
        let moved = snapshot
            .replace(SNAPSHOT, LATEST)
            .replace("\"../../vocab/0.1.0/", "\"../vocab/")
            .replace("\"../../", "\"../");
        for (a, b) in moved.lines().zip(latest.lines()) {
            // A line naming the version IRI as data was rewritten by the
            // substitution above and is not a real difference.
            if a != b && !b.contains("0.1.0") {
                differing.push(format!("{rel}: {a} != {b}"));
            }
        }
        if moved.lines().count() != latest.lines().count() {
            differing.push(format!("{rel}: different length"));
        }
    }
    assert!(
        differing.is_empty(),
        "a snapshot differs from its release beyond its URLs: {:?}",
        &differing[..differing.len().min(3)]
    );
}

#[test]
fn a_snapshot_does_not_contain_itself() {
    let files = built(|_| {});
    assert!(
        !files.keys().any(|p| p.contains("0.1.0/0.1.0")),
        "a release was archived inside a release"
    );
    // Nor does its agent index offer itself as somewhere else to go.
    let llms = &files["vocab/0.1.0/llms.txt"];
    assert!(!llms.contains("Release 0.1.0"));
    assert!(files["vocab/llms.txt"].contains("Release 0.1.0"));
}

#[test]
fn the_policy_decides_what_is_archived() {
    // Off: nothing, and `versions.json` says so rather than going missing.
    let none = built(|c| c.site.snapshots = "none".to_owned());
    assert!(!none.keys().any(|p| p.starts_with("vocab/0.1.0/")));
    let versions: serde_json::Value = serde_json::from_str(&none["versions.json"]).unwrap();
    assert_eq!(versions["policy"], "none");

    // The default archives only where a version IRI points at the snapshot.
    // The category scheme carries no version at all, so it gets nothing
    // under either policy, while the shapes document has a version string
    // but no version IRI and so is archived only under `all`.
    let default = built(|_| {});
    assert!(!default.keys().any(|p| p.starts_with("vocab/shapes/0.1.0/")));
    let all = built(|c| c.site.snapshots = "all".to_owned());
    assert!(
        all.keys().any(|p| p.starts_with("vocab/shapes/0.1.0/")),
        "a versioned namespace was not archived under the `all` policy"
    );
}

#[test]
fn the_version_links_are_rdf_a_reader_can_follow() {
    let files = built(|_| {});
    let ttl = &files["vocab/versions.ttl"];
    assert!(ttl.contains(&format!("<{LATEST}> dcterms:hasVersion <{SNAPSHOT}> .")));
    assert!(ttl.contains(&format!("<{SNAPSHOT}>")));
    assert!(ttl.contains(&format!("dcterms:isVersionOf <{LATEST}>")));
    assert!(ttl.contains("owl:versionInfo \"0.1.0\""));

    // The tool never adds triples to the graph it was given, so the links
    // live beside it rather than inside it.
    assert!(!files["vocab/ex.ttl"].contains("hasVersion"));
}

#[test]
fn a_cache_policy_is_recorded_for_a_host_to_apply() {
    let files = built(|_| {});
    let versions: serde_json::Value = serde_json::from_str(&files["versions.json"]).unwrap();
    // A snapshot never changes and latest changes every release, which is
    // the opposite of how these are usually served.
    assert!(
        versions["cache_control"]["snapshot"]
            .as_str()
            .unwrap()
            .contains("immutable")
    );
    assert!(
        versions["cache_control"]["latest"]
            .as_str()
            .unwrap()
            .contains("must-revalidate")
    );

    let manifest: serde_json::Value = serde_json::from_str(&files["manifest.json"]).unwrap();
    let ns = manifest["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["iri_base"] == LATEST)
        .unwrap();
    assert_eq!(ns["versions"][0]["url"], SNAPSHOT);
    assert!(
        ns["snapshot_cache_control"]
            .as_str()
            .unwrap()
            .contains("immutable")
    );
}
