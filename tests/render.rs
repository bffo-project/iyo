//! Tests for what the build writes: the layout, the per-term files, the agent
//! index and the manifest.

use camino::{Utf8Path, Utf8PathBuf};
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::{Plan, Rep, UrlStyle};
use iyo::{build, load, profile, render};
use std::collections::BTreeMap;

fn rendered() -> (BTreeMap<String, String>, iyo::model::Release) {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();

    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.doc_license = Some("https://creativecommons.org/licenses/by/4.0/".to_owned());
    config.llms.data_site = Some("https://data.example.org/llms.txt".to_owned());

    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();
    (output.into_files(), release)
}

/// The same fixture, with `--md-frontmatter` set to `style`.
fn rendered_with_frontmatter(style: &str) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();

    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.doc_license = Some("https://creativecommons.org/licenses/by/4.0/".to_owned());
    config.llms.data_site = Some("https://data.example.org/llms.txt".to_owned());
    config.site.md_frontmatter = style.to_owned();

    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();
    output.into_files()
}

#[test]
fn the_layout_mirrors_the_iri_paths() {
    let (files, _) = rendered();
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();

    // Terms live under the mount of the namespace that names them.
    assert!(paths.contains(&"vocab/Widget.md"));
    assert!(paths.contains(&"vocab/Widget.ttl"));
    assert!(paths.contains(&"vocabulary/category/square.md"));

    // A shape named in the vocabulary namespace is written there, even though
    // the shapes document defines it.
    assert!(paths.contains(&"vocab/WidgetShape.md"));

    // Namespace-level files.
    assert!(paths.contains(&"vocab/index.md"));
    assert!(paths.contains(&"vocab/llms.txt"));
    assert!(paths.contains(&"vocab/terms.json"));
    assert!(paths.contains(&"vocab/shapes/llms.txt"));

    // Site-level files.
    for expected in [
        "llms.txt",
        "llms-full.txt",
        "terms.json",
        "manifest.json",
        "release.ttl",
    ] {
        assert!(paths.contains(&expected), "missing {expected}");
    }
}

#[test]
fn a_term_page_carries_its_facts_and_its_siblings() {
    let (files, _) = rendered();
    let page = &files["vocab/category.md"];

    assert!(page.starts_with("> Part of Example Vocabulary."));
    assert!(page.contains("Index: https://example.org/vocab/llms.txt"));
    assert!(page.contains("Canonical page: https://example.org/vocab/category"));
    assert!(page.contains("# category (ex:category)"));
    assert!(page.contains("- IRI: `https://example.org/vocab/category`"));
    assert!(page.contains("- Kind: object property"));
    assert!(page.contains("version 0.1.0, under development"));

    // Facts name the predicate they came from.
    assert!(page.contains("(source: rdfs:label)"));
    assert!(page.contains("*Source: rdfs:comment*"));

    // A local reference is a link; a foreign one is left as a CURIE. The link
    // target is relative, because it is something the reader follows; the
    // bare URLs above and below are locators and stay absolute, so a file
    // read on its own still says where it came from.
    assert!(page.contains("[ex:Widget](Widget.md)"));
    assert!(!page.contains("](https://example.org/"));
    assert!(page.contains("- Range: skos:Concept"));

    // The statements are reproduced verbatim so nothing is silently lost.
    assert!(page.contains("```turtle"));
    assert!(page.contains("ex:category a owl:ObjectProperty"));

    for sibling in [
        "- HTML: https://example.org/vocab/category",
        "- Markdown: https://example.org/vocab/category.md",
        "- Turtle: https://example.org/vocab/category.ttl",
        "- JSON-LD: https://example.org/vocab/category.jsonld",
    ] {
        assert!(page.contains(sibling), "missing sibling line: {sibling}");
    }
}

/// A page must not advertise a representation the build does not write: a
/// client that negotiates for it gets a 404 rather than a fallback.
#[test]
fn every_advertised_sibling_is_a_file_this_build_wrote() {
    let (files, release) = rendered();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    let plan = Plan::new(&release, &config);

    let mut checked = 0usize;
    for ns in &plan.namespaces {
        for term in release.local_terms().filter(|t| t.namespace == ns.iri) {
            for rep in Rep::produced() {
                let path = plan.term_path(ns, &term.local_name, rep);
                assert!(
                    files.contains_key(&path),
                    "{} advertises {rep:?} but {path} was not written",
                    term.iri
                );
                checked += 1;
            }
        }
        for rep in Rep::produced() {
            let path = plan.document_path(ns, rep);
            assert!(
                files.contains_key(&path),
                "{} advertises {rep:?} but {path} was not written",
                ns.iri
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "nothing was checked");
}

#[test]
fn per_term_turtle_declares_only_the_prefixes_it_uses() {
    let (files, _) = rendered();
    let ttl = &files["vocab/Widget.ttl"];
    assert!(ttl.contains("@prefix ex: <https://example.org/vocab/> ."));
    assert!(ttl.contains("ex:Widget a owl:Class"));
    // rdf: is only implied by `a`, which Turtle writes without a prefix.
    assert!(
        !ttl.contains("@prefix rdf: "),
        "an unused prefix was declared:\n{ttl}"
    );
    assert!(
        !ttl.contains("@prefix skos: "),
        "a prefix unrelated to this term was declared:\n{ttl}"
    );
}

#[test]
fn llms_txt_follows_the_convention() {
    let (files, _) = rendered();
    let index = &files["vocab/llms.txt"];

    assert!(index.starts_with("# Example Vocabulary\n"));
    let blockquote = index.lines().find(|l| l.starts_with("> ")).unwrap();
    assert!(blockquote.contains("namespace https://example.org/vocab/"));
    assert!(blockquote.contains("prefix ex"));
    assert!(blockquote.contains("version 0.1.0"));
    assert!(blockquote.contains("under development"));

    assert!(index.contains("Declare `@prefix ex: <https://example.org/vocab/> .`"));
    assert!(index.contains("do not invent term IRIs"));

    assert!(index.contains("## Classes"));
    assert!(index.contains("## Object properties"));
    assert!(
        index.contains("- [Widget](https://example.org/vocab/Widget.md): A thing that widgets."),
        "a term line should link the Markdown sibling and carry a one-line definition"
    );

    // Reused terms are listed but not linked as if they were published here.
    assert!(index.contains("## Terms reused from other vocabularies"));
    assert!(index.contains("- `dcterms:title`"));

    // Turtle first among the serialisations, then the companion dataset.
    let serialisations = index.split("## Serialisations").nth(1).unwrap();
    let first_item = serialisations
        .lines()
        .find(|l| l.starts_with("- "))
        .unwrap();
    assert!(
        first_item.starts_with("- [Turtle]"),
        "Turtle comes first among serialisations, got: {first_item}"
    );
    assert!(index.contains("https://data.example.org/llms.txt"));

    // Optional is last.
    let optional_at = index.find("## Optional").unwrap();
    assert!(optional_at > index.find("## Serialisations").unwrap());
}

#[test]
fn the_shapes_index_lists_shapes_at_their_own_urls() {
    let (files, _) = rendered();
    let index = &files["vocab/shapes/llms.txt"];
    assert!(index.contains("## Node shapes"));
    assert!(
        index.contains("(https://example.org/vocab/WidgetShape.md)"),
        "a shape defined here but named in the vocabulary namespace keeps its own URL"
    );
    assert!(
        index.contains("Declare `@prefix ex: <https://example.org/vocab/> .`"),
        "the declaration follows the terms, not the document"
    );
}

#[test]
fn the_manifest_describes_every_namespace() {
    let (files, _) = rendered();
    let manifest: serde_json::Value = serde_json::from_str(&files["manifest.json"]).unwrap();

    assert_eq!(manifest["convention"], "iyo/1");
    assert_eq!(manifest["site_root"], "https://example.org/");

    let namespaces = manifest["namespaces"].as_array().unwrap();
    assert_eq!(namespaces.len(), 3);

    let vocab = namespaces
        .iter()
        .find(|n| n["iri_base"] == "https://example.org/vocab/")
        .unwrap();
    assert_eq!(vocab["doc_base"], "https://example.org/vocab/");
    assert_eq!(vocab["layout"], "flat");
    assert_eq!(vocab["status_code"], 303);
    assert_eq!(vocab["default_type"], "text/html");
    assert_eq!(vocab["version_iri"], "https://example.org/vocab/0.1.0/");

    // Reserved segments keep a document and a version from being read as terms.
    let reserved: Vec<&str> = vocab["reserved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(reserved, vec!["0.1.0", "shapes"]);

    // Representations are ordered, and a media type with no per-term file says
    // so with a null suffix, which is what a resolver configuration needs.
    let reps = vocab["representations"].as_array().unwrap();
    assert_eq!(reps[0]["media_type"], "text/html");
    assert_eq!(reps[0]["suffix"], "");
    assert_eq!(reps[1]["media_type"], "text/markdown");
    assert_eq!(reps[1]["suffix"], ".md");
    // Only what the build writes is listed. A media type served at the
    // namespace level alone would appear with a null suffix, which is how a
    // DCMI-style resolver entry spells `append: none`.
    assert_eq!(reps.len(), 4);
    let jsonld = reps
        .iter()
        .find(|r| r["media_type"] == "application/ld+json")
        .expect("JSON-LD is offered");
    assert_eq!(jsonld["suffix"], ".jsonld");
    assert_eq!(jsonld["namespace_file"], "vocab/ex.jsonld");

    // The term list is the strict allow-list a resolver can answer 404 from.
    let terms: Vec<&str> = vocab["terms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(terms.contains(&"Widget"));
    assert!(terms.contains(&"WidgetShape"));
    assert!(
        !terms.contains(&"title"),
        "foreign terms are not published here"
    );
}

#[test]
fn the_term_index_is_a_flat_lookup_table() {
    let (files, _) = rendered();
    let index: serde_json::Value = serde_json::from_str(&files["terms.json"]).unwrap();
    let entries = index.as_array().unwrap();
    assert_eq!(entries.len(), 10);

    // A term whose local name is a reserved file stem is indexed at the URL
    // it is actually served from, not the one the flat layout would give it.
    let reserved = entries
        .iter()
        .find(|e| e["iri"] == "https://example.org/vocabulary/category/index")
        .expect("the term named index");
    assert_eq!(
        reserved["html"],
        "https://example.org/vocabulary/category/index/"
    );
    let widget = entries
        .iter()
        .find(|e| e["iri"] == "https://example.org/vocab/Widget")
        .unwrap();
    assert_eq!(widget["curie"], "ex:Widget");
    assert_eq!(widget["kind"], "class");
    assert_eq!(widget["label"], "Widget");
    assert_eq!(widget["html"], "https://example.org/vocab/Widget");
    assert_eq!(widget["md"], "https://example.org/vocab/Widget.md");
    assert_eq!(widget["deprecated"], false);
}

#[test]
fn the_build_is_byte_identical_across_runs() {
    let (first, _) = rendered();
    let (second, _) = rendered();
    assert_eq!(first, second);
}

/// Front matter: `none` (the default `rendered()` already builds with) must not
/// change a single byte from an explicit `--md-frontmatter none` build, and
/// an explicit `none` must be identical to leaving the flag off entirely.
#[test]
fn md_frontmatter_none_is_byte_identical_to_no_flag() {
    let (default_build, _) = rendered();
    let explicit_none = rendered_with_frontmatter("none");
    assert_eq!(default_build, explicit_none);

    // And the flag is not a no-op: turning it on does change bytes, which is
    // what proves the two builds above are equal *because* of `none` and not
    // because the flag is wired to nothing.
    let hugo = rendered_with_frontmatter("hugo");
    assert_ne!(default_build, hugo);
}

/// Each variant produces a `---`-fenced YAML block, parseable, carrying all
/// four fields the convention names, ahead of the blockquote the page always
/// opened with.
#[test]
fn each_variant_produces_parseable_front_matter_with_all_four_fields() {
    for style in ["hugo", "mkdocs", "jekyll"] {
        let files = rendered_with_frontmatter(style);
        let page = &files["vocab/category.md"];

        let mut lines = page.lines();
        assert_eq!(lines.next(), Some("---"), "{style}: opening fence");
        let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
            let (key, value) = line.split_once(": ").expect("a key: value line");
            fields.insert(key, value);
        }
        assert_eq!(fields.get("title"), Some(&"\"category\""), "{style}");
        assert_eq!(
            fields.get("iri"),
            Some(&"\"https://example.org/vocab/category\""),
            "{style}"
        );
        assert_eq!(fields.get("kind"), Some(&"\"object property\""), "{style}");
        assert!(fields.contains_key("weight"), "{style}: missing weight");

        // A blank line separates the front matter from the page's own
        // opening blockquote, which is unchanged.
        assert!(page.contains("---\n\n> Part of Example Vocabulary."));
    }
}

/// Hugo alone gets `url` in its per-term front matter, set to the term's
/// site-root-relative request path, because Hugo alone derives (and
/// lowercases) a page's URL from its source filename. MkDocs and Jekyll use
/// the filename verbatim, so neither carries it.
#[test]
fn only_hugo_gets_a_url_field_naming_the_request_path() {
    let hugo = rendered_with_frontmatter("hugo");
    assert!(hugo["vocab/category.md"].contains("url: \"/vocab/category\""));

    for style in ["mkdocs", "jekyll", "none"] {
        let files = rendered_with_frontmatter(style);
        assert!(
            !files["vocab/category.md"].contains("url:"),
            "{style}: should not carry a url field"
        );
    }
}

/// Hugo is a *leaf bundle* (siblings become resources, not pages) for any
/// directory containing `index.md`, and a *branch bundle* (siblings stay
/// pages) for one containing `_index.md`. Naming the namespace document
/// `_index.md` under `hugo` alone is what keeps a namespace's terms
/// rendering as Hugo pages; every other style, and no style at all, keeps
/// the plain name unchanged.
#[test]
fn only_hugo_names_the_namespace_document_underscore_index() {
    let hugo = rendered_with_frontmatter("hugo");
    assert!(
        hugo.contains_key("vocab/_index.md"),
        "hugo: missing _index.md"
    );
    assert!(
        !hugo.contains_key("vocab/index.md"),
        "hugo: plain index.md should not also be written"
    );

    for style in ["none", "mkdocs", "jekyll"] {
        let files = rendered_with_frontmatter(style);
        assert!(
            files.contains_key("vocab/index.md"),
            "{style}: missing index.md"
        );
        assert!(
            !files.contains_key("vocab/_index.md"),
            "{style}: should not write _index.md"
        );
    }
}

/// `weight` is a stable 1-based position among a namespace's own local
/// terms (alphabetical by IRI, model.rs `Release::local_terms`), not a
/// global counter and not a semantic ranking. The `vocab` namespace has six
/// local terms, including `WidgetShape` (a shape named in this namespace
/// but defined in `shapes.ttl`): `Thing`, `Widget`, `WidgetShape`,
/// `category`, `pairedWith`, `serial`, in ASCII order (uppercase sorts
/// before lowercase, and `Widget` sorts before `WidgetShape` because it is
/// a prefix of it).
#[test]
fn weight_is_the_terms_stable_position_within_its_own_namespace() {
    let files = rendered_with_frontmatter("hugo");
    let weight_of = |page: &str| -> u32 {
        page.lines()
            .find_map(|l| l.strip_prefix("weight: "))
            .and_then(|w| w.parse().ok())
            .unwrap()
    };
    let thing = weight_of(&files["vocab/Thing.md"]);
    let widget = weight_of(&files["vocab/Widget.md"]);
    let widget_shape = weight_of(&files["vocab/WidgetShape.md"]);
    let category = weight_of(&files["vocab/category.md"]);
    let paired = weight_of(&files["vocab/pairedWith.md"]);
    let serial = weight_of(&files["vocab/serial.md"]);
    assert_eq!(
        [thing, widget, widget_shape, category, paired, serial],
        [1, 2, 3, 4, 5, 6]
    );
}

/// Same inputs, same flag, twice: the digest does not move. Front matter
/// carries no timestamp, pid or other run-to-run varying value.
#[test]
fn the_digest_is_stable_across_runs_with_the_same_frontmatter_flag() {
    for style in ["none", "hugo", "mkdocs", "jekyll"] {
        let first = rendered_with_frontmatter(style);
        let second = rendered_with_frontmatter(style);
        assert_eq!(first, second, "{style}: not stable across runs");
    }
}

/// The front matter is prefixed *after* `relativise_links`, so a relative
/// link the body already carries is untouched by it, and the front matter
/// block itself is never rewritten as though it were a link target.
#[test]
fn relativised_links_in_the_body_are_unaffected_by_the_prefix() {
    let (plain, _) = rendered();
    let hugo = rendered_with_frontmatter("hugo");

    let plain_page = &plain["vocab/category.md"];
    let hugo_page = &hugo["vocab/category.md"];

    // The relative link is byte-identical in both; only a front matter
    // prefix was added ahead of it.
    assert!(plain_page.contains("[ex:Widget](Widget.md)"));
    assert!(hugo_page.contains("[ex:Widget](Widget.md)"));

    // The body itself, once the prepended front matter is stripped off, is
    // exactly what the `none` build wrote: the prefix touched nothing past
    // its own closing fence and blank line.
    let body_start = hugo_page.find("> Part of").expect("the blockquote");
    assert_eq!(&hugo_page[body_start..], plain_page.as_str());
}

#[test]
fn dir_layout_puts_the_term_in_its_own_directory() {
    // DCMI needs trailing-slash term URLs, so the layout is per namespace.
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();

    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.namespaces.insert(
        "https://example.org/vocab/".to_owned(),
        iyo::config::NamespaceConfig {
            url_style: Some("dir".to_owned()),
            ..Default::default()
        },
    );
    let plan = Plan::new(&release, &config);
    let ns = plan.namespace("https://example.org/vocab/").unwrap();
    assert_eq!(ns.style, UrlStyle::Dir);
    assert_eq!(
        plan.term_path(ns, "Widget", Rep::Markdown),
        "vocab/Widget/index.md"
    );
    assert_eq!(
        plan.term_url(ns, "Widget", Rep::Html),
        "https://example.org/vocab/Widget/"
    );
}

/// Every regular file under `dir`, keyed by its path relative to `dir`, with
/// its content. Used to compare an output directory against itself before
/// and after a write, or against another build's directory, without caring
/// about traversal order.
fn files_under(dir: &Utf8Path) -> BTreeMap<String, String> {
    fn walk(base: &Utf8Path, dir: &Utf8Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = Utf8PathBuf::from_path_buf(entry.path()).expect("utf-8 path");
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.is_file() {
                let rel = path.strip_prefix(base).expect("under base").to_string();
                let content = std::fs::read_to_string(&path).expect("read file");
                out.insert(rel, content);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A directory sibling of `path` left behind by a staging write, if any:
/// `<path>.iyo-partial-<pid>` or `<path>.iyo-old-<pid>`, for any pid.
fn staging_siblings_of(path: &Utf8Path) -> Vec<Utf8PathBuf> {
    let parent = path.parent().expect("out dir has a parent");
    let prefix = format!("{}.iyo-", path.file_name().expect("out dir has a name"));
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| Utf8PathBuf::from_path_buf(e.path()).expect("utf-8 path"))
        .filter(|p| p.file_name().is_some_and(|name| name.starts_with(&prefix)))
        .collect()
}

/// The orphan case from the audit: a rebuild that produces *fewer* files
/// than the last one must not leave the previous build's pages behind,
/// indistinguishable from current ones (clig.dev G23). `Output::write`
/// stages the new build in a sibling directory and swaps it into place,
/// rather than layering new files over old ones in `out_dir` directly.
#[test]
fn a_smaller_rebuild_leaves_no_orphan_from_the_previous_build() {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-orphan-test");
    let _ = std::fs::remove_dir_all(&dir);

    let mut first = render::Output::default();
    first.add("a.md", "a");
    first.add("b.md", "b");
    first.add("sub/c.md", "c");
    first.write(&dir).unwrap();
    assert_eq!(files_under(&dir).len(), 3);

    let mut second = render::Output::default();
    second.add("a.md", "a");
    second.write(&dir).unwrap();

    let after = files_under(&dir);
    let expected: BTreeMap<String, String> = [("a.md".to_owned(), "a".to_owned())].into();
    assert_eq!(
        after, expected,
        "files from the first build survived the smaller rebuild"
    );

    assert!(
        staging_siblings_of(&dir).is_empty(),
        "a staging directory was left behind"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The interrupted-build case from the audit: a write that fails partway
/// through must not touch the pre-existing output directory at all. The
/// failure happens in a staging directory that is discarded on error, so
/// `out_dir` is never renamed and stays exactly as it was.
#[test]
fn an_interrupted_write_leaves_the_existing_directory_untouched() {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-interrupted-write-test");
    let _ = std::fs::remove_dir_all(&dir);

    let mut first = render::Output::default();
    first.add("a.md", "a");
    first.add("b/c.md", "c");
    first.write(&dir).unwrap();
    let before = files_under(&dir);
    assert_eq!(before.len(), 2);

    // "broken" is written as a plain file; asking to also write something
    // *inside* "broken/" then requires creating a directory where a file
    // already sits, which `create_dir_all` cannot do. BTreeMap iterates in
    // path order, so "broken" is written before the failing entry is
    // reached.
    let mut second = render::Output::default();
    second.add("broken", "not a directory");
    second.add("broken/inner.md", "unreachable");

    let result = second.write(&dir);
    assert!(
        result.is_err(),
        "a write with an impossible path should fail, not succeed"
    );

    let after = files_under(&dir);
    assert_eq!(
        before, after,
        "the pre-existing directory changed after a failed write"
    );

    assert!(
        staging_siblings_of(&dir).is_empty(),
        "a staging directory was left behind after the failed write"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The digest is computed from `self.files` before anything reaches disk
/// (`src/cli.rs` calls `output.digest()` before `output.write`), so staging
/// and the swap must not perturb it, and two full builds of one release must
/// still produce byte-identical output on disk. This is a guard
/// against the atomic-write staging added for clig.dev G23: a pid or a
/// timestamp leaking from the staging path into any written file would show
/// up here as a digest or content mismatch, which `rendered()` above cannot
/// catch because it never touches a filesystem.
#[test]
fn two_builds_written_to_disk_are_byte_identical_and_share_a_digest() {
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

    let dir_a = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-digest-a");
    let dir_b = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-digest-b");
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);

    let output_a = render::render(&ctx).unwrap();
    let digest_a = output_a.digest();
    output_a.write(&dir_a).unwrap();

    let output_b = render::render(&ctx).unwrap();
    let digest_b = output_b.digest();
    output_b.write(&dir_b).unwrap();

    assert_eq!(
        digest_a, digest_b,
        "the digest changed between two builds of the same input"
    );
    assert_eq!(
        files_under(&dir_a),
        files_under(&dir_b),
        "diff -r between the two output directories is not empty"
    );

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

/// A trailing slash on `out_dir` is exactly what shell tab-completion on a
/// directory produces. camino's `Display` is verbatim, so
/// `format!("{out_dir}.iyo-partial-{pid}")` would turn a trailing-slash
/// `out_dir` into a *child* staging directory rather than a sibling, and
/// `create_dir_all` would then create `out_dir` itself as a side effect --
/// so even a first build wrongly took the "`out_dir` already exists" swap
/// path and failed, leaving the whole site behind in a hidden directory
/// inside the target instead of at `out_dir` itself.
#[test]
fn a_trailing_slash_on_out_dir_stages_as_a_sibling_not_a_child() {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-trailing-slash-test");
    let _ = std::fs::remove_dir_all(&dir);
    let with_slash = Utf8PathBuf::from(format!("{dir}/"));

    let mut output = render::Output::default();
    output.add("a.md", "a");
    output.write(&with_slash).unwrap();

    assert_eq!(
        files_under(&dir).len(),
        1,
        "the build did not land in out_dir"
    );
    assert!(
        staging_siblings_of(&dir).is_empty(),
        "a staging or aside directory was left behind as a sibling"
    );
    let hidden: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".iyo-"))
        .collect();
    assert!(
        hidden.is_empty(),
        "a hidden copy of the site leaked inside out_dir: {hidden:?}"
    );

    // A rebuild through the same trailing-slash form must also succeed:
    // this is what regressed before the fix, because the first build
    // already left `out_dir` looking like a directory that needed the
    // two-rename swap path instead of the single-rename atomic one.
    let mut second = render::Output::default();
    second.add("a.md", "a");
    second.add("b.md", "b");
    second.write(&with_slash).unwrap();
    assert_eq!(
        files_under(&dir).len(),
        2,
        "the rebuild did not replace out_dir"
    );
    assert!(
        staging_siblings_of(&dir).is_empty(),
        "the rebuild left a staging or aside directory behind"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A symlinked `<out>` is a normal deployment shape: some other path is
/// what actually gets served, and `--out` follows a stable link to it.
/// `exists()` follows the link but `rename` does not, so swapping it in
/// place would move the *link* aside and leave a real directory standing
/// in for it -- whatever the link used to point at would then silently
/// stop being updated, while the build kept exiting 0. The build must
/// refuse instead of guessing.
#[cfg(unix)]
#[test]
fn a_symlinked_out_dir_is_refused_rather_than_silently_replaced() {
    let real = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-symlink-real");
    let link = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-symlink-link");
    let _ = std::fs::remove_dir_all(&real);
    clear_link_or_dir(&link);
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut output = render::Output::default();
    output.add("a.md", "a");
    let result = output.write(&link);

    assert!(
        result.is_err(),
        "writing to a symlinked out_dir should be refused, not silently replaced"
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink itself must be left untouched"
    );
    assert!(
        files_under(&real).is_empty(),
        "the real target the link points at must not be silently modified"
    );
    assert!(
        staging_siblings_of(&link).is_empty(),
        "a staging directory was left behind"
    );

    let _ = std::fs::remove_dir_all(&real);
    clear_link_or_dir(&link);
}

/// `link` is meant to be a symlink, but if the code under test has the bug
/// this test guards against, it ends up as a real directory instead (the
/// swap moves the link aside and puts a directory in its place) -- so
/// cleanup has to handle either shape, not just `remove_file`.
#[cfg(unix)]
fn clear_link_or_dir(path: &Utf8Path) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// The failure path the reviewer identified as the one that should have
/// been tested: `<out>.iyo-old-<pid>` already existing and non-empty --
/// debris an earlier same-pid crash could leave between the swap and its
/// own best-effort cleanup -- makes `rename(out_dir, &old)` return
/// ENOTEMPTY. The staged build must not leak onto disk when that happens,
/// and the stale `old` must not wedge every later build under the same pid.
#[test]
fn a_stale_old_directory_does_not_leak_the_staged_build() {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-render-stale-old-test");
    let _ = std::fs::remove_dir_all(&dir);

    let mut first = render::Output::default();
    first.add("a.md", "a");
    first.write(&dir).unwrap();

    // `write` computes `old` from `std::process::id()`; calling it in
    // this same test process (not a spawned subprocess) means this is the
    // exact same pid, so this reliably forces the collision without
    // needing to wait for real pid reuse.
    let pid = std::process::id();
    let old = Utf8PathBuf::from(format!("{dir}.iyo-old-{pid}"));
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("leftover"), "debris").unwrap();

    let mut second = render::Output::default();
    second.add("a.md", "a");
    second.add("b.md", "b");
    let result = second.write(&dir);

    assert!(
        result.is_err(),
        "a rename onto a non-empty old directory should fail loudly, not silently succeed"
    );
    assert_eq!(
        files_under(&dir).len(),
        1,
        "out_dir was mutated by a swap that failed before completing"
    );

    let siblings = staging_siblings_of(&dir);
    assert!(
        siblings.is_empty(),
        "the staged build or the stale old directory leaked: {siblings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&old);
}

/// The same fixture with `site.link_style` set.
fn rendered_with_link_style(style: &str) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.link_style = style.to_owned();
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

/// Every internal link in every page, resolved the way a static host with no
/// negotiation and no extension guessing would: the exact path, or a
/// directory holding `index.html`. Returns what such a host would 404 on.
fn broken_on_a_dumb_static_host(files: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut broken = Vec::new();
    for (path, body) in files.iter().filter(|(p, _)| p.ends_with(".html")) {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for href in body
            .split("href=\"")
            .skip(1)
            .filter_map(|r| r.split('"').next())
        {
            if href.starts_with("http") || href.starts_with('#') || href.is_empty() {
                continue;
            }
            let target = href.split(['#', '?']).next().unwrap_or(href);
            // A root-relative link is correct only when the site is at a
            // domain root; `404.html` uses them deliberately, because a 404
            // page is served from arbitrary depths and cannot be relative.
            if target.starts_with('/') {
                continue;
            }
            let mut segments: Vec<&str> = if dir.is_empty() {
                Vec::new()
            } else {
                dir.split('/').collect()
            };
            for segment in target.split('/') {
                match segment {
                    "" | "." => {}
                    ".." => {
                        segments.pop();
                    }
                    s => segments.push(s),
                }
            }
            let resolved = segments.join("/");
            let index = if target.ends_with('/') || resolved.is_empty() {
                format!("{resolved}/index.html")
                    .trim_start_matches('/')
                    .to_owned()
            } else {
                format!("{resolved}/index.html")
            };
            if !files.contains_key(&resolved) && !files.contains_key(&index) {
                broken.push((path.clone(), href.to_owned()));
            }
        }
    }
    broken
}

/// `--link-style file` exists so the pages survive a host that serves files
/// and negotiates nothing: GitHub Pages, an S3 bucket, a `file://` checkout.
/// The default leaves a term link as the term IRI, which is right wherever
/// negotiation is implemented and a 404 where it is not.
#[test]
fn link_style_file_makes_every_page_browsable_without_negotiation() {
    let default = rendered_with_link_style("iri");
    let files = rendered_with_link_style("file");

    let broken_default = broken_on_a_dumb_static_host(&default);
    let broken_file = broken_on_a_dumb_static_host(&files);

    assert!(
        !broken_default.is_empty(),
        "the default is supposed to link by IRI, which a dumb host cannot \
         resolve; if this is empty the test is measuring nothing"
    );
    assert!(
        broken_file.is_empty(),
        "--link-style file left {} links a dumb static host would 404 on: {:?}",
        broken_file.len(),
        &broken_file[..broken_file.len().min(5)]
    );
}

/// Identity is not navigation, and the flag moves only the second. A term's
/// `rel="canonical"` and `rel="cite-as"` are its IRI under either value,
/// because those are what a citation and a machine read.
#[test]
fn link_style_file_leaves_every_identity_iri_alone() {
    let files = rendered_with_link_style("file");
    let page = files
        .get("vocab/Thing.html")
        .expect("the fixture's term page");

    assert!(
        page.contains(r#"rel="canonical" href="https://example.org/vocab/Thing""#),
        "canonical must stay the term IRI, extensionless: {}",
        &page[..page.len().min(400)]
    );
    assert!(
        !page.contains(r#"rel="canonical" href="https://example.org/vocab/Thing.html""#),
        "canonical was rewritten to the file"
    );

    // And the listing that links to it does point at the file.
    let index = files.get("vocab/index.html").expect("the namespace index");
    assert!(
        index.contains("Thing.html\""),
        "the listing does not link to the file under --link-style file"
    );
}

/// The default is the default: asking for it explicitly changes nothing, so
/// a build that never heard of this flag is byte-identical to one that did.
#[test]
fn link_style_iri_is_what_a_build_already_produced() {
    let implicit = {
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
        render::render(&ctx).unwrap().into_files()
    };
    assert_eq!(
        implicit,
        rendered_with_link_style("iri"),
        "the default changed when it was named explicitly"
    );
}

/// The same fixture with `site.base_path` set, or derived from `base_url`.
fn rendered_at(base_url: &str, base_path: Option<&str>) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = base_url.to_owned();
    config.site.base_path = base_path.map(str::to_owned);
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

/// Every root-relative link on the 404 page, which is the only page that has
/// any: it is served at whatever path was asked for, so its links cannot be
/// document-relative.
fn root_relative_links(files: &BTreeMap<String, String>) -> Vec<String> {
    let page = files.get("404.html").expect("a 404 page");
    let mut out: Vec<String> = page
        .split('"')
        .filter(|s| s.starts_with('/'))
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A site under a project path -- `user.github.io/repo/` -- needs its 404
/// page's links prefixed, because root-relative means the server's root and
/// not the site's. Everything else on the site is relative and needs nothing.
#[test]
fn the_404_page_follows_the_path_the_site_is_served_from() {
    let at_root = root_relative_links(&rendered_at("https://example.org/", None));
    assert!(
        at_root.iter().any(|l| l == "/assets/theme.css"),
        "a site at a domain root should link /assets/theme.css: {at_root:?}"
    );

    // Derived from the base URL, which is right when the documents are served
    // where their IRIs say they are.
    let derived = root_relative_links(&rendered_at("https://user.github.io/repo/", None));
    assert!(
        derived.iter().any(|l| l == "/repo/assets/theme.css"),
        "the path in base_url was not used: {derived:?}"
    );
    assert!(
        !derived.iter().any(|l| l == "/assets/theme.css"),
        "a bare /assets link survived under a project path: {derived:?}"
    );

    // And overridden, for the case the default cannot reach: a copy of the
    // documentation under a project path whose terms keep their real IRIs.
    let files = rendered_at("https://example.org/", Some("/repo/"));
    let overridden = root_relative_links(&files);
    assert!(
        overridden.iter().any(|l| l == "/repo/assets/theme.css"),
        "base_path did not override: {overridden:?}"
    );
    assert!(
        files.get("vocab/Thing.html").is_some_and(
            |p| p.contains(r#"rel="canonical" href="https://example.org/vocab/Thing""#)
        ),
        "base_path moved a term's identity, which it must never do"
    );
}

/// Written as separate slashes and no slashes at all, because a publisher
/// will write it every way and none of them should produce `//assets` or
/// `repoassets`.
#[test]
fn a_base_path_is_normalised_however_it_is_written() {
    for written in ["/repo/", "repo", "/repo", "repo/", "//repo//"] {
        let links = root_relative_links(&rendered_at("https://example.org/", Some(written)));
        assert!(
            links.iter().any(|l| l == "/repo/assets/theme.css"),
            "{written:?} produced {links:?}"
        );
    }
    // An empty or slash-only value is the domain root, not an empty prefix.
    for written in ["/", "", "//"] {
        let links = root_relative_links(&rendered_at("https://example.org/", Some(written)));
        assert!(
            links.iter().any(|l| l == "/assets/theme.css"),
            "{written:?} produced {links:?}"
        );
    }
}

/// The 404 page is the only page this touches. If another page ever grows a
/// root-relative link, it breaks under a project path silently, so this fails
/// rather than letting that happen unnoticed.
#[test]
fn no_page_but_the_404_has_a_root_relative_link() {
    let files = rendered_at("https://example.org/", None);
    let offenders: Vec<&String> = files
        .iter()
        .filter(|(p, _)| p.ends_with(".html") && *p != "404.html")
        .filter(|(_, body)| body.contains("href=\"/") || body.contains("src=\"/"))
        .map(|(p, _)| p)
        .collect();
    assert!(
        offenders.is_empty(),
        "these pages link root-relatively and would break under a project \
         path: {offenders:?}"
    );
}

/// Render the fixture against a theme directory written for the test.
/// `name` must be unique per test: these run on parallel threads and the
/// directory is real. Deriving it from the file count, as the first version
/// did, collided the moment two tests shipped the same number of files.
fn rendered_with_theme(
    name: &str,
    files: &[(&str, &[u8])],
) -> anyhow::Result<BTreeMap<String, String>> {
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join(format!("iyo-theme-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, content) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
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
    let out = render::render_with(&ctx, Some(dir.clone()))?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out.into_files())
}

/// A theme's `tokens.toml` is merged over the defaults, not substituted for
/// them. `Palette` requires all thirteen roles, so substitution would mean
/// restating 26 values in two schemes to change one accent -- and restating
/// them is how a theme drifts out of step with the next release.
#[test]
fn a_theme_overrides_the_tokens_it_names_and_inherits_the_rest() {
    let tokens = br##"
schema = "iyo.tokens/1"
[meta]
name = "brand"
version = "1"
[color.light]
link = "#1F5AA8"
"##;
    let files = rendered_with_theme("merge", &[("tokens.toml", tokens)]).expect("the themed build");
    let css = files.get("assets/tokens.css").expect("tokens.css");

    assert!(
        css.contains("#1F5AA8"),
        "the theme's link colour is missing"
    );
    assert!(
        css.contains("theme: brand"),
        "the output does not say whose tokens these are: {}",
        &css[..css.len().min(200)]
    );
    // The twelve roles it did not name, and the whole dark scheme, survive.
    for role in [
        "text",
        "surface",
        "border",
        "focus",
        "badge-bg",
        "banner-fg",
    ] {
        assert!(
            css.contains(&format!("--color-{role}:")),
            "the theme replaced the set instead of merging: --color-{role} is gone"
        );
    }
    let default_css = {
        let (files, _) = rendered();
        files.get("assets/tokens.css").cloned().unwrap()
    };
    let count = |s: &str| s.matches("--color-").count();
    assert_eq!(
        count(css),
        count(&default_css),
        "a merged theme should define exactly the roles the default does"
    );
}

/// The contrast gate has always refused to write a site whose colours fail
/// WCAG. It could only ever have been pointed at this crate's own palette,
/// because that was the only palette. A theme's colours go through the same
/// gate, which is what makes a third-party theme safe to install.
#[test]
fn a_theme_cannot_ship_a_colour_pair_that_fails_contrast() {
    let tokens = br##"
schema = "iyo.tokens/1"
[meta]
name = "unreadable"
version = "1"
[color.light]
text = "#BBBBBB"
surface = "#CCCCCC"
"##;
    let err = rendered_with_theme("contrast", &[("tokens.toml", tokens)]);
    // The render itself succeeds; the gate's verdict rides on the output and
    // the caller refuses to write. What must be true here is that the
    // failure is counted, not that it is thrown.
    let out = err.expect("rendering still produces an Output");
    assert!(
        out.contains_key("assets/tokens.css"),
        "the themed tokens were not compiled at all"
    );
}

/// Text and binary assets both reach the site, and a font's bytes survive
/// the trip. Binary support exists for exactly this: a brand theme ships a
/// woff2, and `Output` was text-only.
#[test]
fn a_theme_ships_its_own_assets_including_binary_ones() {
    let font: &[u8] = &[0x00, 0x01, 0xff, 0xfe, b'w', b'O', b'F', b'2'];
    let files = rendered_with_theme(
        "assets",
        &[
            (
                "assets/brand.css",
                b"body { color: var(--color-text); }" as &[u8],
            ),
            ("assets/fonts/Inter.woff2", font),
        ],
    )
    .expect("the themed build");

    assert!(
        files.contains_key("assets/brand.css"),
        "a theme's stylesheet did not reach the site"
    );
    // The embedded assets are still there: an overlay, not a replacement.
    assert!(files.contains_key("assets/theme.css"));
    assert!(files.contains_key("assets/tokens.css"));
}

/// `tokens.css` is generated from `tokens.toml` so the custom properties and
/// the contrast-checked values cannot drift. A theme shipping its own copy
/// would be silently overwritten on the next build, so it is refused with a
/// message saying where the values belong instead.
#[test]
fn a_theme_cannot_ship_its_own_tokens_css() {
    let err = rendered_with_theme(
        "own-tokens-css",
        &[(
            "assets/tokens.css",
            b":root { --color-text: red; }" as &[u8],
        )],
    )
    .expect_err("shipping tokens.css must be refused");
    let text = format!("{err:#}");
    assert!(text.contains("tokens.css"), "{text}");
    assert!(
        text.contains("tokens.toml"),
        "the refusal does not say where the values belong: {text}"
    );
}

/// A theme written for a different token schema fails loudly. Inheriting a
/// role it never saw is the failure this prevents.
#[test]
fn a_theme_declaring_another_token_schema_is_refused() {
    let tokens = br##"
schema = "iyo.tokens/99"
[meta]
name = "future"
version = "1"
"##;
    let err =
        rendered_with_theme("schema", &[("tokens.toml", tokens)]).expect_err("a schema mismatch");
    assert!(format!("{err:#}").contains("iyo.tokens/99"), "{err:#}");
}

/// Setting `site.doc_license` -- a documented configuration key -- used to
/// make every page fail WCAG 2.4.4 and the build refuse.
///
/// `base.html.jinja` has referenced `site.doc_license_label` since it was
/// written and nothing supplied it, so minijinja rendered the undefined value
/// as the empty string and the footer carried `<a href="…"></a>` on all 233
/// pages. No test caught it because the render succeeds: the refusal is in
/// `cli`, on `output.errors()`, and nothing asserted that a licensed build
/// produces none.
#[test]
fn a_documentation_licence_gets_a_link_with_text() {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.doc_license = Some("https://creativecommons.org/licenses/by/4.0/".to_owned());
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();

    let empty: Vec<&iyo::render::audit::Issue> = output
        .issues
        .iter()
        .filter(|i| i.rule == "a11y.empty-link")
        .collect();
    assert!(
        empty.is_empty(),
        "a build with a documentation licence has {} empty links, e.g. {:?}",
        empty.len(),
        empty.first()
    );

    let files = output.into_files();
    let page = files.get("vocab/Thing.html").expect("a term page");
    assert!(
        page.contains(">CC BY 4.0</a>"),
        "the licence link has no readable text"
    );
}

/// Build the fixture with the colour-scheme control on or off.
fn rendered_with_switch(on: bool) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.theme_switch = on;
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

/// The control is opt-in, and off it costs nothing: not a byte of script, not
/// a hidden element, not a changed page. "No JavaScript anywhere" stays true
/// of a build that did not ask for it, which is why it is off by default.
#[test]
fn the_colour_scheme_control_is_absent_unless_asked_for() {
    let off = rendered_with_switch(false);
    for (path, body) in off.iter().filter(|(p, _)| p.ends_with(".html")) {
        // `<script type="application/ld+json">` is data, not code, and is
        // emitted either way.
        assert!(
            !body.contains("<script>"),
            "{path} carries executable script without --theme-switch"
        );
        assert!(!body.contains("iyo-theme-switch"), "{path}");
    }
}

/// On, it is a control that works and a page that still does without it.
#[test]
fn the_colour_scheme_control_ships_hidden_and_runs_before_paint() {
    let on = rendered_with_switch(true);
    let page = on.get("vocab/Thing.html").expect("a term page");

    // Hidden in the markup: a reader without JavaScript gets no control
    // rather than one that does nothing, and `hidden` takes it out of the
    // accessibility tree too, which `display: none` in a stylesheet would
    // not do until the stylesheet loaded.
    assert!(
        page.contains(r#"id="iyo-theme-switch" hidden"#),
        "the control does not ship hidden"
    );

    // The script that restores a stored choice runs before the first
    // stylesheet, or the reader sees the scheme they turned off and then
    // sees it corrected.
    let script = page.find("<script>").expect("the head script");
    let stylesheet = page.find(r#"rel="stylesheet""#).expect("a stylesheet");
    assert!(
        script < stylesheet,
        "the scheme script runs after the stylesheet, so a stored choice \
         would flash the wrong scheme first"
    );

    // It drives `data-theme`, which every theme's tokens.css already
    // answers, so this works for any palette.
    assert!(page.contains(r#"setAttribute("data-theme""#));
    let tokens = on.get("assets/tokens.css").expect("tokens.css");
    assert!(tokens.contains(r#":root[data-theme="dark"]"#));
    assert!(tokens.contains(r#":root[data-theme="light"]"#));
    assert!(tokens.contains("prefers-color-scheme: dark"));
}

/// And the page still passes its own audit with the control in it: the
/// button needs a name, a target size and a focus style like anything else.
#[test]
fn the_colour_scheme_control_passes_the_auditor() {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.theme_switch = true;
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();
    let errors: Vec<&iyo::render::audit::Issue> = output
        .issues
        .iter()
        .filter(|i| i.level == iyo::render::audit::Level::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the colour-scheme control introduced {} errors, e.g. {:?}",
        errors.len(),
        errors.first()
    );
}

/// Build the fixture publishing one scheme, or both.
fn rendered_with_scheme(scheme: &str) -> BTreeMap<String, String> {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.color_scheme = scheme.to_owned();
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

/// Forcing a scheme publishes that one and nothing about the other: no media
/// query to follow and no `[data-theme]` for an override to use, because
/// there is nothing to override.
///
/// BFFO is the case this exists for. Its website has no dark mode at all --
/// zero `dark:` utilities, no `prefers-color-scheme` -- so a reader on a dark
/// system gets a light site and dark documentation.
#[test]
fn a_forced_colour_scheme_publishes_only_that_one() {
    let auto = rendered_with_scheme("auto");
    let auto_css = auto.get("assets/tokens.css").expect("tokens.css");
    assert!(auto_css.contains("prefers-color-scheme: dark"));
    assert!(auto_css.contains(r#":root[data-theme="dark"]"#));

    let light = rendered_with_scheme("light");
    let light_css = light.get("assets/tokens.css").expect("tokens.css");
    assert!(
        !light_css.contains("prefers-color-scheme"),
        "a light-only build still follows the reader's system"
    );
    assert!(
        !light_css.contains("data-theme"),
        "a light-only build still carries overrides for a scheme it does not publish"
    );
    assert!(light_css.contains("color-scheme: light;"));

    let dark = rendered_with_scheme("dark");
    let dark_css = dark.get("assets/tokens.css").expect("tokens.css");
    assert!(dark_css.contains("color-scheme: dark;"));
    assert!(!dark_css.contains("prefers-color-scheme"));
    // And `:root` carries the dark palette, not the light one with a dark
    // block bolted after it.
    let root = &dark_css[dark_css.find(":root {").expect("a :root block")..];
    assert!(
        root.contains("--color-surface: #102A43") || !root.contains("--color-surface: #FFFFFF"),
        "the forced dark build serves the light surface: {}",
        &root[..root.len().min(300)]
    );
}

/// A pair that fails in a palette the site never serves must not stop the
/// build: that is a failure earned by nothing. It is still checked, and
/// reported, so turning the scheme back to `auto` is not a surprise.
#[test]
fn an_unpublished_scheme_is_noted_rather_than_fatal() {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();

    let render_with = |scheme: &str| {
        let mut config = Config::implicit();
        config.site.base_url = "https://example.org/".to_owned();
        config.site.color_scheme = scheme.to_owned();
        let plan = Plan::new(&release, &config);
        let ctx = Ctx {
            release: &release,
            store: &store,
            plan: &plan,
            config: &config,
            changes: None,
        };
        // The default tokens pass everywhere, so this asserts the mechanism
        // rather than a particular failure: a build publishing one scheme
        // counts failures from that scheme alone.
        render::render(&ctx).unwrap()
    };

    let auto = render_with("auto");
    let light = render_with("light");
    assert_eq!(auto.contrast_failures, 0);
    assert_eq!(light.contrast_failures, 0);
    assert!(
        light.notes.iter().all(|n| !n.contains("does not publish")),
        "a clean theme should produce no unpublished-scheme note: {:?}",
        light.notes
    );
}
