//! Tests for the release comparison.
//!
//! The thing worth asserting is not that a change is noticed but that it is
//! filed correctly. A changelog exists so a consumer can decide whether to
//! upgrade, and a list that calls a tightened constraint "additive" is worse
//! than no list, because it will be believed.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::diff::{self, Severity};
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, load, model::Release, profile, render};
use std::collections::BTreeMap;

fn release(dir: &str) -> (Release, load::Store) {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&[dir.to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    (build::build(&store, &registry).unwrap(), store)
}

fn compare() -> diff::Diff {
    let (old, _) = release("testdata/previous");
    let (new, _) = release("testdata/mini");
    diff::run(&old, &new)
}

fn find<'a>(d: &'a diff::Diff, rule: &str) -> Vec<&'a diff::Change> {
    d.changes.iter().filter(|c| c.rule == rule).collect()
}

#[test]
fn a_removed_term_is_breaking_and_keeps_its_anchor() {
    let d = compare();
    let removed = find(&d, "term.removed");
    assert_eq!(removed.len(), 1);
    let change = removed[0];
    assert_eq!(
        change.iri.as_deref(),
        Some("https://example.org/vocab/legacyCode")
    );
    assert_eq!(change.severity, Severity::Breaking);
    // Without the anchor there is nowhere for a link into the old release to
    // land, because the term's page is gone.
    assert_eq!(change.anchor.as_deref(), Some("legacyCode"));
    assert!(
        change
            .detail
            .contains("An identifier kept from an older system")
    );
}

#[test]
fn an_added_term_breaks_nothing() {
    let d = compare();
    let added = find(&d, "term.added");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].severity, Severity::Additive);
    assert!(
        added[0]
            .iri
            .as_deref()
            .is_some_and(|i| i.ends_with("rounded_square"))
    );
}

#[test]
fn tightening_a_constraint_is_breaking_and_loosening_it_is_not() {
    // `ex:category` was optional and is now required: records that validated
    // stop validating. Nothing in the RDF vocabulary changed, so a diff that
    // read only the triples would report nothing at all here.
    let forwards = compare();
    let tightened = find(&forwards, "constraint.now-required");
    assert_eq!(tightened.len(), 1);
    assert_eq!(tightened[0].severity, Severity::Breaking);
    assert!(tightened[0].detail.contains("optional to required"));

    // The same change read the other way round is additive.
    let (old, _) = release("testdata/mini");
    let (new, _) = release("testdata/previous");
    let backwards = diff::run(&old, &new);
    let loosened = find(&backwards, "constraint.now-optional");
    assert_eq!(loosened.len(), 1);
    assert_eq!(loosened[0].severity, Severity::Additive);
}

#[test]
fn wording_changes_are_editorial() {
    let d = compare();
    for rule in ["term.relabelled", "term.redefined"] {
        let found = find(&d, rule);
        assert_eq!(found.len(), 1, "{rule} was not reported");
        assert_eq!(found[0].severity, Severity::Editorial);
        assert!(
            found[0]
                .iri
                .as_deref()
                .is_some_and(|i| i.ends_with("serial"))
        );
    }
}

#[test]
fn comparing_a_release_with_itself_finds_nothing() {
    let (r, _) = release("testdata/mini");
    let d = diff::run(&r, &r);
    assert!(d.is_empty());
    assert_eq!(d.breaking, 0);
    assert!(diff::markdown(&d, "Example").contains("Nothing changed"));
}

#[test]
fn the_changelog_leads_with_what_breaks() {
    let d = compare();
    let md = diff::markdown(&d, "Example Vocabulary");
    let breaking = md.find("## Breaking").expect("a breaking section");
    let additive = md.find("## Additive").expect("an additive section");
    let editorial = md.find("## Editorial").expect("an editorial section");
    assert!(breaking < additive && additive < editorial);
    // A removed term's old anchor is in the file itself, so the changelog
    // answers the fragment wherever it is rendered.
    assert!(md.contains("<a id=\"legacyCode\"></a>"));
}

#[test]
fn a_build_given_a_previous_release_publishes_the_changelog() {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let new = build::build(&store, &registry).unwrap();
    let (old, _) = release("testdata/previous");
    let changes = diff::run(&old, &new);

    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    let plan = Plan::new(&new, &config);
    let ctx = Ctx {
        release: &new,
        store: &store,
        plan: &plan,
        config: &config,
        changes: Some(&changes),
    };
    let files: BTreeMap<String, String> = render::render(&ctx).unwrap().into_files();

    // One changelog per namespace that actually changed, and none for the
    // namespaces that did not.
    assert!(files.contains_key("vocab/changes.md"));
    let changelog = &files["vocab/changes.md"];
    assert!(changelog.contains("legacyCode"));
    // A scoped changelog must not quote release-wide term totals beside one
    // namespace's changes.
    assert!(!changelog.contains("terms before"));

    // The page accounts for the identifier that no longer has a page.
    let page = &files["vocab/index.html"];
    assert!(page.contains("id=\"legacyCode\""));
    assert!(page.contains("Terms no longer here"));
    assert!(page.contains("changes.md"));
    assert!(files["vocab/llms.txt"].contains("changes.md"));

    // A build with no previous release writes none of it.
    let plain = Ctx {
        changes: None,
        ..ctx
    };
    let without: BTreeMap<String, String> = render::render(&plain).unwrap().into_files();
    assert!(!without.keys().any(|p| p.ends_with("changes.md")));
    assert!(!without["vocab/index.html"].contains("Terms no longer here"));
}
