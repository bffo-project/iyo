//! Tests for the HTML layer: the theme overlay, escaping, ids and determinism.
//!
//! Every case here stands for a defect that shipped and was measured, not for
//! a hypothetical one. Escaped markup and dead custom properties are both
//! silent: the page stays valid HTML and the browser shows something
//! plausible, so neither a validator nor an in-browser checker reports them.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, load, profile, render};
use std::collections::BTreeMap;

fn render_theme(theme: Option<Utf8PathBuf>) -> BTreeMap<String, String> {
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
    render::render_with(&ctx, theme).unwrap().into_files()
}

fn pages(files: &BTreeMap<String, String>) -> Vec<(&String, &String)> {
    files.iter().filter(|(p, _)| p.ends_with(".html")).collect()
}

#[test]
fn no_page_shows_markup_as_text() {
    let files = render_theme(None);
    let mut offenders = Vec::new();
    for (path, html) in pages(&files) {
        // Outside a code block, an escaped tag means a template escaped
        // markup it meant to emit. Eight fact rows shipped that way.
        let outside: String = html
            .split("<pre")
            .enumerate()
            .filter(|(i, _)| *i == 0)
            .map(|(_, s)| s.to_owned())
            .collect();
        let tail: String = html
            .split("</pre>")
            .skip(1)
            .map(|s| s.split("<pre").next().unwrap_or("").to_owned())
            .collect();
        for chunk in [outside, tail] {
            for opener in ["&lt;code", "&lt;a ", "&lt;span", "&lt;div"] {
                if chunk.contains(opener) {
                    offenders.push(format!("{path}: {opener}"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "markup escaped into text: {offenders:?}"
    );
}

#[test]
fn every_custom_property_the_theme_reads_is_defined() {
    let files = render_theme(None);
    let tokens = files.get("assets/tokens.css").expect("tokens.css written");
    let sheets: Vec<&str> = files
        .iter()
        .filter(|(p, _)| p.ends_with(".css") && p.as_str() != "assets/tokens.css")
        .map(|(_, c)| c.as_str())
        .collect();
    assert!(!sheets.is_empty(), "no stylesheet was written");
    assert_eq!(
        iyo::theme::undefined_tokens(&sheets, tokens),
        Vec::<String>::new(),
        "a colour was proved against WCAG and then never reached the page"
    );
}

#[test]
fn structural_ids_are_namespaced_against_term_names() {
    let files = render_theme(None);
    // A term called `metadata` collided with the page section of that name
    // until every structural id gained a prefix.
    for (path, html) in pages(&files) {
        for id in ["id=\"iyo-facts\"", "id=\"iyo-main\""] {
            if html.contains("iyo-facts") {
                assert!(
                    html.contains(id) || !html.contains(&id[4..]),
                    "{path} uses an unprefixed structural id"
                );
            }
        }
    }
    let term = files
        .get("vocabulary/category/square.html")
        .expect("a term page");
    assert!(term.contains("id=\"iyo-facts\""));
}

#[test]
fn a_term_named_index_keeps_its_own_page() {
    let files = render_theme(None);
    // Both exist, and they are different documents.
    let scheme = files
        .get("vocabulary/category/index.html")
        .expect("the scheme page");
    let term = files
        .get("vocabulary/category/index/index.html")
        .expect("the page of the term named index");
    assert!(scheme.contains("Example Categories"));
    assert!(term.contains("Index"));
    assert_ne!(scheme, term);
    // Its siblings live beside it, not beside the scheme's.
    assert!(files.contains_key("vocabulary/category/index/index.ttl"));
    assert!(files.contains_key("vocabulary/category/index/index.md"));
}

#[test]
fn a_theme_directory_overrides_a_bundled_template() {
    let dir = std::env::temp_dir().join("iyo-theme-overlay-test");
    let templates = dir.join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(
        templates.join("404.html.jinja"),
        "{% extends \"base.html.jinja\" %}\n\
         {% block title %}Nothing here{% endblock %}\n\
         {% block main %}<h1>Nothing here</h1><p>Overlaid.</p>{% endblock %}\n",
    )
    .unwrap();

    let overlaid = render_theme(Some(Utf8PathBuf::from_path_buf(dir.clone()).unwrap()));
    let page = overlaid.get("404.html").expect("the 404 page");
    assert!(page.contains("Nothing here"), "the overlay was not used");

    // Only the overridden template changes; the rest still comes from the
    // bundled theme, which is what makes an overlay cheap to maintain.
    let bundled = render_theme(None);
    assert_eq!(
        bundled.get("vocabulary/category/square.html"),
        overlaid.get("vocabulary/category/square.html")
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_renders_produce_the_same_bytes() {
    assert_eq!(render_theme(None), render_theme(None));
}
