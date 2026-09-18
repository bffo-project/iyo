//! Tests for the host adapters.
//!
//! The claim these support is that a host configuration is a pure function of
//! the manifest, so the first test
//! is that the function is pure in the literal sense, and the rest are about
//! the two failure modes that cost a working site: a request that resolves to
//! the wrong file, and a request that resolves to itself.
//!
//! What each adapter *does* is checked by the scripts in `tests/hosts/`,
//! which run the generated Worker in Node and apply the generated `.htaccess`
//! to a request matrix. Structure is checked here because it runs on every
//! build; behaviour is checked there because it needs another runtime.

use camino::Utf8PathBuf;
use iyo::adapter::{self, Host};
use iyo::config::Config;
use iyo::render::{Ctx, manifest};
use iyo::site::Plan;
use iyo::{build, load, profile};

fn manifest_of(base_url: &str) -> manifest::Manifest {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
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
    manifest::build(&ctx)
}

fn file(files: &[(String, String)], ends_with: &str) -> String {
    files
        .iter()
        .find(|(p, _)| p.ends_with(ends_with))
        .unwrap_or_else(|| panic!("{ends_with} was not emitted"))
        .1
        .clone()
}

#[test]
fn every_host_is_a_function_of_the_manifest_alone() {
    let manifest = manifest_of("https://example.org/");
    for host in Host::ALL {
        let once = adapter::emit(&manifest, host);
        let twice = adapter::emit(&manifest, host);
        assert_eq!(once, twice, "{} is not deterministic", host.as_str());
        assert!(!once.is_empty(), "{} emitted nothing", host.as_str());
        // Every file lands under the host's own directory, so two hosts can
        // be emitted side by side without either overwriting the other.
        for (path, _) in &once {
            assert!(
                path.starts_with(&format!("adapters/{}/", host.as_str())),
                "{path} escaped its adapter directory"
            );
        }
    }
}

#[test]
fn a_serving_host_never_redirects_a_page_to_itself() {
    // The document origin is the same as the term origin, so Apache is
    // serving the files. Redirecting `/vocab/Widget` to `/vocab/Widget` is an
    // infinite redirect, and it is what the first version of this adapter
    // did.
    let files = adapter::emit(&manifest_of("https://example.org/"), Host::Apache);
    let htaccess = file(&files, ".htaccess");
    for line in htaccess.lines().filter(|l| l.starts_with("RewriteRule")) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (pattern, target, flags) = (parts[1], parts[2], parts.get(3).copied().unwrap_or(""));
        if !flags.contains("R=") {
            continue;
        }
        // A redirecting rule must send the request somewhere else: a target
        // that is the matched path with the anchors stripped is a loop.
        let bare = pattern.trim_start_matches('^').trim_end_matches("/?$");
        assert_ne!(
            target.trim_start_matches('/'),
            bare,
            "this rule redirects to what it matched: {line}"
        );
    }
    // A page is answered, not redirected.
    assert!(
        htaccess.contains("/vocab/$1.html [L]"),
        "a serving host should rewrite a page internally"
    );
}

#[test]
fn a_redirect_host_sends_every_representation_onwards() {
    // The documents are on another origin, which is the w3id arrangement.
    let files = adapter::emit(&manifest_of("https://docs.example.org/"), Host::Apache);
    let htaccess = file(&files, ".htaccess");
    assert!(htaccess.contains("w3id.org checkout"));
    assert!(
        htaccess.contains("https://docs.example.org/vocab/$1 [R=303,L]"),
        "the page should be redirected to the document host"
    );
    assert!(!htaccess.contains("[L]\nRewriteRule ^vocab/?$ /vocab/index.html"));
}

#[test]
fn a_term_whose_file_is_a_directory_gets_its_own_rules() {
    // `index` is a concept in the fixture and also the scheme's own page, so
    // it is served from a directory. An adapter that used the general rule
    // would send it to the scheme.
    let manifest = manifest_of("https://example.org/");
    let htaccess = file(&adapter::emit(&manifest, Host::Apache), ".htaccess");
    assert!(htaccess.contains("/vocabulary/category/index/index.ttl"));

    let vercel = file(&adapter::emit(&manifest, Host::Vercel), "vercel.json");
    let config: serde_json::Value = serde_json::from_str(&vercel).unwrap();
    let redirects = config["redirects"].as_array().unwrap();
    let has_dir_rule = redirects.iter().any(|r| {
        r["destination"]
            .as_str()
            .is_some_and(|d| d == "/vocabulary/category/index/index.ttl")
    });
    assert!(has_dir_rule, "vercel has no rule for the directory term");

    // And the general rule must not swallow a sibling: a dynamic segment
    // matches `Widget.ttl` unless the pattern says otherwise.
    let general = redirects
        .iter()
        .find(|r| r["source"].as_str().is_some_and(|s| s.contains(":local")))
        .expect("a general term rule");
    assert!(
        general["source"].as_str().unwrap().contains("[^./]"),
        "the dynamic segment would match a file name with an extension"
    );
}

#[test]
fn the_worker_carries_its_table_and_the_headers_the_convention_requires() {
    let files = adapter::emit(&manifest_of("https://example.org/"), Host::Cloudflare);
    let worker = file(&files, "worker.js");
    // The table is embedded, so the Worker needs no subrequest to route.
    assert!(worker.contains("const TABLE = {"));
    assert!(worker.contains("\"terms\":[\"Thing\""));
    for required in [
        "rel=\"canonical\"",
        "rel=\"cite-as\"",
        "rel=\"alternate\"",
        "rel=\"describedby\"",
    ] {
        assert!(worker.contains(required), "the Worker omits {required}");
    }
    assert!(worker.contains("Vary"));
    assert!(worker.contains("Access-Control-Allow-Origin"));

    // Cloudflare allows 100 rules and 2,000 characters a line.
    let headers = file(&files, "_headers");
    let rules = headers.lines().filter(|l| l.starts_with('/')).count();
    assert!(
        rules <= 100,
        "{rules} header rules is over Cloudflare's limit"
    );
    assert!(headers.lines().all(|l| l.len() <= 2000));
}

#[test]
fn the_dcmi_entry_is_a_narrowing_of_the_manifest() {
    let manifest = manifest_of("https://example.org/");
    let files = adapter::emit(&manifest, Host::DcmiNs);
    let entry: serde_json::Value = serde_json::from_str(&file(&files, "resolver/ex.json")).unwrap();
    assert_eq!(entry["namespace"], "https://example.org/vocab/");
    assert_eq!(entry["resolverType"], "strict");
    // `suffix: null` in the manifest is this schema's `append: "none"`.
    let reps = entry["representations"].as_array().unwrap();
    assert!(reps.iter().all(|r| r["append"].is_string()));
    // The term allow-list survives, which is what `strict` means.
    assert!(
        entry["terms"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Widget"))
    );
}

/// The GitHub Pages notes name the file that actually resolves. A term that
/// falls back to the directory layout lives at `<term>/index.html`, and the
/// note used to say `<term>.html`, which on this fixture is the *namespace's
/// own* index page: a different resource, answering 200. A wrong URL that
/// resolves to something real is worse than one that 404s.
#[test]
fn the_github_notes_name_the_file_that_resolves_not_a_guess() {
    let manifest = manifest_of("https://example.org/");
    let notes = file(
        &adapter::emit(&manifest, Host::GitHubPages),
        "RESOLUTION.md",
    );
    assert!(
        notes.contains("https://example.org/vocabulary/category/index/index.html"),
        "the directory-layout term is not named at its real file: {notes}"
    );
    assert!(
        !notes.contains("`https://example.org/vocabulary/category/index.html`"),
        "the note still points at the namespace document: {notes}"
    );
    // A flat term is unaffected.
    assert!(notes.contains("https://example.org/vocab/Thing.html"));
    // On this fixture the directory-layout term happens to sort first, so
    // the example line above is the one that names it. When it does not
    // sort first -- the usual case, and BFFO's -- it still gets a line of
    // its own, because it is the term a reader would guess wrong about.
    let mut shifted = manifest_of("https://example.org/");
    for ns in &mut shifted.namespaces {
        if !ns.dir_terms.is_empty() {
            ns.terms.insert(0, "Aardvark".to_owned());
        }
    }
    let notes = file(&adapter::emit(&shifted, Host::GitHubPages), "RESOLUTION.md");
    assert!(
        notes.contains("https://example.org/vocabulary/category/Aardvark.html"),
        "the example line no longer names the first term: {notes}"
    );
    assert!(
        notes.contains("its file is a directory")
            && notes.contains("https://example.org/vocabulary/category/index/index.html"),
        "a directory-layout term that does not sort first got no line: {notes}"
    );
    // A namespace with no terms says so rather than naming an invented one.
    assert!(notes.contains("mints no terms"), "{notes}");
    assert!(
        !notes.contains("/Term"),
        "the placeholder term IRI is back: {notes}"
    );
}

/// The notes told readers a term IRI does **not** resolve on GitHub Pages. It
/// does: Pages answers an extensionless path with the matching `.html` file,
/// and redirects a directory to its trailing slash. Measured on a real
/// deployment, `/demo/Instrument` answered 200, not 404.
///
/// The wrong claim survived because every other test here checks which *file*
/// the note names, and none checked what the note said would happen to the
/// IRI. A note that names the right file and describes the wrong response is
/// still wrong, so this asserts the claim rather than the path.
#[test]
fn the_github_notes_do_not_claim_a_term_iri_fails_to_resolve() {
    let manifest = manifest_of("https://example.org/");
    let notes = file(
        &adapter::emit(&manifest, Host::GitHubPages),
        "RESOLUTION.md",
    );
    assert!(
        !notes.contains("does **not** resolve"),
        "the note says term IRIs do not resolve; Pages answers them with HTML: {notes}"
    );
    // A flat term answers 200 outright; a directory one arrives via a 301.
    assert!(notes.contains("answers 200 with"), "{notes}");
    assert!(
        notes.contains("answers 301 to"),
        "a directory-layout term is not described as a redirect: {notes}"
    );
    // The real limitation still has to be stated, or the correction has simply
    // traded one misleading note for another.
    assert!(
        notes.contains("ignores `Accept`") && notes.contains("only ever to HTML"),
        "the note no longer states what Pages cannot do: {notes}"
    );
}

/// The `dcmi-ns` schema cannot express a per-term layout, so the projection
/// carries the list and the README says what a resolver built from it would
/// get wrong. Dropping the list, which is what this adapter used to do, made
/// the loss invisible: the entry looked complete.
#[test]
fn the_dcmi_entry_carries_the_layout_it_cannot_express() {
    let manifest = manifest_of("https://example.org/");
    let files = adapter::emit(&manifest, Host::DcmiNs);

    let with_dir: serde_json::Value =
        serde_json::from_str(&file(&files, "resolver/ex-cat.json")).unwrap();
    assert_eq!(
        with_dir["dirTerms"],
        serde_json::json!(["index"]),
        "the directory-layout term is missing from the projection"
    );

    let without: serde_json::Value =
        serde_json::from_str(&file(&files, "resolver/ex.json")).unwrap();
    assert_eq!(without["dirTerms"], serde_json::json!([]));

    let readme = file(&files, "README.md");
    assert!(
        readme.contains("cannot express a per-term layout"),
        "{readme}"
    );
    assert!(
        readme.contains(
            "  - `/vocabulary/category/`: `https://example.org/vocabulary/category/index`"
        ),
        "the README does not name the namespace and the term at risk, as a nested \
         list item: {readme}"
    );
}

/// And the warning is about *this* manifest, not a fixed sentence: a release
/// with no directory-layout term anywhere must not carry it.
#[test]
fn a_manifest_with_no_directory_terms_gets_no_such_warning() {
    let mut manifest = manifest_of("https://example.org/");
    for ns in &mut manifest.namespaces {
        ns.dir_terms.clear();
    }
    let readme = file(&adapter::emit(&manifest, Host::DcmiNs), "README.md");
    assert!(
        !readme.contains("cannot express a per-term layout"),
        "a warning fired with nothing to warn about: {readme}"
    );
    // The host's permanent limits are still there.
    assert!(readme.contains("No per-term suffix"));
}

#[test]
fn every_adapter_says_what_it_cannot_do() {
    let manifest = manifest_of("https://example.org/");
    for host in Host::ALL {
        let readme = file(&adapter::emit(&manifest, host), "README.md");
        assert!(readme.contains("What this host cannot do"));
        for note in host.degradations() {
            // The first few words are enough to know the note reached the file.
            let head: String = note
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                readme.contains(&head),
                "{}'s README omits {head:?}",
                host.as_str()
            );
        }
    }
}
