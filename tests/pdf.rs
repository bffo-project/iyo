//! Tests for the PDF stage.
//!
//! Split the way the feature is: the inputs are part of the build and are
//! checked here on every run, and compiling them needs Typst, so those checks
//! skip when it is absent rather than failing on a machine that has no reason
//! to have it.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, load, pdf, profile, render};
use std::collections::BTreeMap;

fn built(with_pdf: bool) -> (BTreeMap<String, String>, std::collections::BTreeSet<String>) {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    let mut config = Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    config.site.pdf = with_pdf;
    let plan = Plan::new(&release, &config);
    let ctx = Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    let output = render::render(&ctx).unwrap();
    let promised = output.promised.clone();
    (output.into_files(), promised)
}

#[test]
fn the_inputs_are_written_and_the_template_reads_them() {
    let (files, _) = built(true);
    let spec = files
        .get("vocab/pdf/spec.typ")
        .expect("the template was not written");
    let model = files
        .get("vocab/pdf/model.json")
        .expect("the data was not written");

    // The host language generates no Typst markup: the template is a file a
    // publisher can edit, and it reads the data rather than being generated
    // from it.
    assert!(spec.contains("json(\"model.json\")"));
    assert!(spec.contains("#set document("));
    // PDF/A needs a title, and the tagged structure needs a language.
    assert!(spec.contains("title: doc.title"));
    assert!(spec.contains("lang: lang"));

    let parsed: serde_json::Value = serde_json::from_str(model).unwrap();
    assert_eq!(parsed["document"]["title"], "Example Vocabulary");
    assert_eq!(parsed["pdf"]["stem"], "ex");
    assert!(
        !parsed["document"]["sections"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn nothing_is_written_when_no_pdf_was_asked_for() {
    let (files, promised) = built(false);
    assert!(!files.keys().any(|p| p.contains("/pdf/")));
    assert!(promised.is_empty());
    // And the page does not offer one.
    assert!(!files["vocab/index.html"].contains(".pdf"));
}

#[test]
fn a_page_may_link_the_pdf_because_the_build_promised_it() {
    let (files, promised) = built(true);
    // The renderer does not write the PDF; Typst does, afterwards. The
    // promise is what lets the auditor tell that apart from a broken link,
    // and it is what caught a snapshot page linking a PDF nobody compiled.
    assert!(promised.contains("vocab/ex.pdf"));
    // The href is relative, like every other navigation link, so that the
    // same build serves from the origin it names and from a local preview.
    // `vocab/index.html` is one directory down, hence the `../`.
    assert!(files["vocab/index.html"].contains(r#"href="../vocab/ex.pdf""#));
    assert!(!files["vocab/index.html"].contains("https://example.org/vocab/ex.pdf"));
    assert!(files["vocab/llms.txt"].contains("vocab/ex.pdf"));
}

#[test]
fn every_promised_pdf_has_inputs_to_compile_it_from() {
    let (files, promised) = built(true);
    assert!(!promised.is_empty());
    for path in &promised {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let spec = if dir.is_empty() {
            "pdf/spec.typ".to_owned()
        } else {
            format!("{dir}/pdf/spec.typ")
        };
        assert!(
            files.contains_key(&spec),
            "{path} was promised but {spec} was not written"
        );
    }
}

#[test]
fn typst_compiles_the_inputs_into_a_tagged_pdf() {
    if !pdf::available("typst") {
        eprintln!("skipped: typst is not on PATH");
        return;
    }
    let (files, _) = built(true);
    let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .unwrap()
        .join("iyo-pdf-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, content) in &files {
        if !path.starts_with("vocab/pdf/") {
            continue;
        }
        let full = dir.join(path.rsplit_once('/').map(|(_, f)| f).unwrap_or(path));
        std::fs::write(full, content).unwrap();
    }

    let output = dir.join("ex.pdf");
    let built = pdf::compile(
        &dir,
        &output,
        pdf::timestamp(Some("2026-04-28")),
        &pdf::Options::default(),
    )
    .expect("typst could not compile the bundled template");
    assert!(built.bytes > 1000);

    let raw = std::fs::read(&output).unwrap();
    // Tagged structure is what a screen reader follows, and it is the whole
    // reason for asking Typst for a standard rather than a plain PDF.
    for marker in [
        &b"/StructTreeRoot"[..],
        &b"/Marked true"[..],
        &b"pdfaid"[..],
        &b"/Lang"[..],
    ] {
        assert!(
            raw.windows(marker.len()).any(|w| w == marker),
            "the PDF has no {}",
            String::from_utf8_lossy(marker)
        );
    }

    // The creation date comes from the vocabulary, so two compiles of one
    // release give the same bytes.
    let again = dir.join("ex-again.pdf");
    pdf::compile(
        &dir,
        &again,
        pdf::timestamp(Some("2026-04-28")),
        &pdf::Options::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&output).unwrap(),
        std::fs::read(&again).unwrap(),
        "two compiles of one release differ"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_typst_is_reported_rather_than_guessed_at() {
    assert!(!pdf::available("/nonexistent/typst"));
    let dir = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let error = pdf::compile(
        &dir,
        &dir.join("nowhere.pdf"),
        None,
        &pdf::Options {
            binary: "/nonexistent/typst".to_owned(),
            ..pdf::Options::default()
        },
    )
    .unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("Install Typst"), "{message}");
    assert!(message.contains("--typst-bin"), "{message}");
}
