//! The negotiation contract, checked against the Rust implementation.
//!
//! The cases are the bundled `tests/hosts/conformance.json` contract,
//! resolved against the fixture manifest by `conform::resolve`, exactly as
//! `iyo conform` itself does. The JavaScript the Cloudflare adapter generates
//! is checked against the same contract, resolved the same way, so both
//! implementations are measured against a written specification rather than
//! against each other: agreeing on a wrong answer still fails.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::negotiate::{self, Outcome};
use iyo::render::{Ctx, manifest};
use iyo::site::Plan;
use iyo::{build, load, profile};

fn manifest_of() -> manifest::Manifest {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
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
    manifest::build(&ctx)
}

#[test]
fn the_resolver_satisfies_the_written_contract() {
    let manifest = manifest_of();
    let contract = iyo::conform::cases::bundled().unwrap();
    let plan = iyo::conform::resolve::resolve(&contract, &manifest);
    assert!(
        plan.unresolved.is_empty(),
        "unresolved roles: {:?}",
        plan.unresolved
    );

    let mut failures = Vec::new();
    let mut checked = 0usize;
    let mut skipped = 0usize;
    for case in &plan.cases {
        use iyo::conform::resolve::ResolvedExpect as E;
        let got = negotiate::resolve(
            &manifest,
            &case.path,
            case.accept.as_deref(),
            case.query.as_deref(),
        );
        let ok = match &case.expect {
            // IMPORTANT 3: comparing only `media_type` here is tautological.
            // `negotiate::resolve` constructs `Outcome::Serve` exactly when
            // `chosen.media_type == ns.default_type`, and every `serve` case
            // in the contract expects `text/html`, which is the default on
            // every fixture namespace -- so the comparison could never fail;
            // the arm was really still testing "the resolver returned
            // `Serve`". Comparing `file` too is what the predecessor test
            // did (pinning exact output paths like `vocab/Widget.html` and
            // `vocabulary/category/index/index.html`, the dir-term
            // case), and is the one field that actually distinguishes a
            // wrong resolution from a right one.
            E::Serve {
                media_type: m,
                file: f,
                ..
            } => {
                matches!(&got, Outcome::Serve { media_type: g, file } if g == m && file == f)
            }
            E::Redirect {
                location, status, ..
            } => matches!(
                &got,
                Outcome::Redirect { location: l, status: s } if l == location && s == status
            ),
            // The resolver has no filesystem, so it cannot tell a file that
            // exists from one that does not. Both are PassThrough here, and
            // the distinction is checked by the harnesses that can see it.
            E::File { .. } | E::Absent => {
                skipped += 1;
                matches!(got, Outcome::PassThrough)
            }
        };
        checked += 1;
        if !ok {
            failures.push(format!(
                "{}: got {got:?}, wanted {:?}",
                case.name, case.expect
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        checked >= 26,
        "fewer cases ran than the matrix had: {checked}"
    );
    eprintln!("{checked} cases, {skipped} whose kind this layer cannot distinguish");
}

#[test]
fn a_link_header_names_every_relation_signposting_asks_for() {
    let manifest = manifest_of();
    let ns = manifest
        .namespaces
        .iter()
        .find(|n| n.mount == "/vocab/")
        .unwrap();
    let chosen = negotiate::select(ns, Some("text/turtle"), None);
    let link = negotiate::link_header(&manifest, ns, "Widget", chosen);

    for relation in ["canonical", "cite-as", "alternate", "describedby"] {
        assert!(
            link.contains(&format!("rel=\"{relation}\"")),
            "no {relation}"
        );
    }
    // `cite-as` names the identity IRI, not the page it is served from: that
    // distinction is the whole point of the relation.
    assert!(link.contains("<https://example.org/vocab/Widget>; rel=\"cite-as\""));
    // The chosen representation is not listed as an alternate of itself.
    assert!(!link.contains("rel=\"alternate\"; type=\"text/turtle\""));

    // Identity is absolute wherever it is read. What the client is expected
    // to fetch is a relative reference, resolved against the request URI, so
    // one response is correct from the origin the manifest names and from a
    // preview on some other host.
    assert!(link.contains("<https://example.org/vocab/Widget>; rel=\"canonical\""));
    assert!(link.contains("<Widget.md>; rel=\"alternate\"; type=\"text/markdown\""));
    assert!(link.contains("<llms.txt>; rel=\"describedby\""));
    assert!(
        !link.contains("<https://example.org/vocab/Widget.md>"),
        "an alternate still names an origin: {link}"
    );
}

/// The whole header, byte for byte, against `expected_link`, which the
/// resolver computes from the manifest for every `serve` and `redirect`
/// case. A role-based contract cannot write the header literally, because it
/// contains resolved paths -- but the resolved plan can, and asserting the
/// relations are merely present says nothing about where they point, which
/// is the half that a release once got wrong (commit 125bf97: `describedby`
/// pointing at its parent's `llms.txt` instead of its own).
#[test]
fn the_link_header_matches_the_written_contract() {
    let manifest = manifest_of();
    let contract = iyo::conform::cases::bundled().unwrap();
    let plan = iyo::conform::resolve::resolve(&contract, &manifest);

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for case in &plan.cases {
        let Some(want) = &case.expected_link else {
            continue;
        };
        let (ns, local) =
            negotiate::target(&manifest, &case.path).expect("a namespace for the path");
        // Mirrors how `serve.rs` derives the override from a request's query
        // string, so this drives `select`/`link_header` the same way a real
        // response does rather than skipping negotiation's query handling.
        let override_type = case.query.as_deref().and_then(|q| {
            q.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                matches!(key, "format" | "_mediatype" | "_profile").then(|| value.to_owned())
            })
        });
        let chosen = negotiate::select(&ns, case.accept.as_deref(), override_type.as_deref());
        let got = negotiate::link_header(&manifest, &ns, &local, chosen);
        checked += 1;
        if &got != want {
            failures.push(format!("{}:\n  got  {got}\n  want {want}", case.name));
        }
    }
    assert!(checked > 0, "no case carried an expected_link to check");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
