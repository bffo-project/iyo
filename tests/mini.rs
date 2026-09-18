//! End-to-end tests over `testdata/mini`, a three-file release that exercises
//! the parts of a real one that are easy to get wrong: a nested document IRI,
//! a version IRI under the namespace, a term defined in a file other than the
//! one that mints its namespace, a reused term declared in place, and a SKOS
//! hierarchy given only with `skos:topConceptOf`.

use camino::Utf8PathBuf;
use iyo::model::{DocumentKind, TermKind};
use iyo::{build, check, load, profile};

fn mini() -> (iyo::model::Release, load::Store) {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).expect("inputs resolve");
    assert_eq!(paths.len(), 3, "three fixture files");
    let store = load::load(&paths).expect("fixture parses");
    let registry = profile::Registry::built_in().expect("profiles load");
    let release = build::build(&store, &registry).expect("model builds");
    (release, store)
}

#[test]
fn documents_and_profiles() {
    let (release, _) = mini();
    assert_eq!(release.documents.len(), 3);

    let vocab = release.document("https://example.org/vocab/").unwrap();
    assert_eq!(vocab.kind, DocumentKind::Ontology);
    assert_eq!(vocab.profile, "dcap");

    let scheme = release
        .document("https://example.org/vocabulary/category/")
        .unwrap();
    assert_eq!(scheme.kind, DocumentKind::Scheme);
    assert_eq!(
        scheme.profile, "skos",
        "a concept scheme that is also typed owl:Ontology is still SKOS"
    );

    let shapes = release
        .document("https://example.org/vocab/shapes/")
        .unwrap();
    assert_eq!(shapes.kind, DocumentKind::Shapes);
    assert_eq!(
        shapes.profile, "shacl",
        "a shapes file typed owl:Ontology is still shapes"
    );
}

#[test]
fn header_is_read_through_the_profile() {
    let (release, _) = mini();
    let vocab = release.document("https://example.org/vocab/").unwrap();
    let h = &vocab.header;
    assert_eq!(h.title[0].value, "Example Vocabulary");
    assert_eq!(h.version_info.as_deref(), Some("0.1.0"));
    assert_eq!(
        h.version_iri.as_deref(),
        Some("https://example.org/vocab/0.1.0/")
    );
    assert_eq!(h.prefix.as_deref(), Some("ex"));
    assert_eq!(
        h.status.as_deref(),
        Some("http://purl.org/adms/status/UnderDevelopment")
    );
    assert_eq!(h.creators.len(), 1);
    assert_eq!(h.creators[0].name.as_deref(), Some("A Curator"));
    assert_eq!(
        h.creators[0].kind.as_deref(),
        Some("http://xmlns.com/foaf/0.1/Person"),
        "a creator given as a blank node keeps its type"
    );
}

#[test]
fn a_term_belongs_to_the_document_that_defines_it() {
    let (release, _) = mini();
    let shape = release
        .term("https://example.org/vocab/WidgetShape")
        .unwrap();
    assert_eq!(shape.kind, TermKind::NodeShape);
    assert_eq!(
        shape.namespace, "https://example.org/vocab/",
        "the shape is named in the vocabulary namespace"
    );
    assert_eq!(
        shape.defined_in.as_deref(),
        Some("https://example.org/vocab/shapes/"),
        "but it is defined in the shapes document, by source file"
    );
    assert!(!shape.foreign);
}

#[test]
fn reused_terms_are_foreign_whatever_their_type() {
    let (release, _) = mini();
    let title = release.term("http://purl.org/dc/terms/title").unwrap();
    assert!(title.foreign, "dcterms: is not minted by this release");
    assert_eq!(title.curie.as_deref(), Some("dcterms:title"));
    assert_eq!(
        title.anchor, "dcterms_title",
        "foreign anchors are prefixed, and never contain a colon"
    );
    assert!(
        !title.definitions.is_empty(),
        "its usage note is carried over"
    );

    let local: Vec<&str> = release.local_terms().map(|t| t.iri.as_str()).collect();
    assert!(!local.contains(&"http://purl.org/dc/terms/title"));
}

#[test]
fn reserved_segments_come_from_nested_documents_and_the_version_iri() {
    let (release, _) = mini();
    let ns = release
        .namespaces
        .iter()
        .find(|n| n.iri == "https://example.org/vocab/")
        .unwrap();
    assert_eq!(ns.prefix.as_deref(), Some("ex"));
    assert_eq!(ns.reserved, vec!["0.1.0".to_owned(), "shapes".to_owned()]);
}

#[test]
fn skos_hierarchy_uses_top_concept_of_and_inverts_broader() {
    let (release, _) = mini();
    let square = release
        .term("https://example.org/vocabulary/category/square")
        .unwrap();
    assert_eq!(square.kind, TermKind::Concept);
    assert_eq!(
        square.concept.top_concept_of,
        vec!["https://example.org/vocabulary/category/".to_owned()]
    );
    assert_eq!(
        square.concept.narrower,
        vec!["https://example.org/vocabulary/category/rounded_square".to_owned()],
        "narrower is inverted from broader"
    );
    assert_eq!(square.concept.notation.as_deref(), Some("square"));
    assert_eq!(square.labels[0].value, "Square");
    assert_eq!(square.labels[0].lang.as_deref(), Some("en"));

    let rounded = release
        .term("https://example.org/vocabulary/category/rounded_square")
        .unwrap();
    assert_eq!(rounded.mappings.len(), 1);
    assert_eq!(
        rounded.mappings[0].relation,
        "http://www.w3.org/2004/02/skos/core#relatedMatch"
    );
}

#[test]
fn property_facts_and_characteristics() {
    let (release, _) = mini();
    let paired = release
        .term("https://example.org/vocab/pairedWith")
        .unwrap();
    assert_eq!(paired.kind, TermKind::ObjectProperty);
    assert_eq!(
        paired.property.characteristics,
        vec!["symmetric".to_owned()]
    );
    let widget = release.term("https://example.org/vocab/Widget").unwrap();
    assert_eq!(
        widget.super_terms,
        vec!["https://example.org/vocab/Thing".to_owned()]
    );
    let thing = release.term("https://example.org/vocab/Thing").unwrap();
    assert_eq!(
        thing.sub_terms,
        vec!["https://example.org/vocab/Widget".to_owned()],
        "sub-terms are inverted from super-terms"
    );
}

#[test]
fn counts() {
    let (release, _) = mini();
    let s = &release.stats;
    assert_eq!(s.files, 3);
    assert_eq!(s.documents, 3);
    assert_eq!(s.terms_foreign, 1);
    assert_eq!(s.by_kind.get("class"), Some(&2));
    assert_eq!(s.by_kind.get("object property"), Some(&2));
    assert_eq!(s.by_kind.get("datatype property"), Some(&1));
    assert_eq!(s.by_kind.get("concept"), Some(&4));
    assert_eq!(s.by_kind.get("node shape"), Some(&1));
    assert_eq!(s.terms_local, 10);
}

#[test]
fn check_finds_what_the_fixture_plants() {
    let (release, store) = mini();
    let report = check::run(&release, &store, &check::Options::default());
    let rules: Vec<&str> = report.findings.iter().map(|f| f.rule.as_str()).collect();

    // An unused prefix is declared in vocab.ttl.
    assert!(rules.contains(&"release.prefix-unused"));
    // The shapes document is not listed in the root's dcterms:hasPart.
    assert!(rules.contains(&"release.has-part-missing"));
    // The parts carry no version IRI of their own.
    assert!(rules.contains(&"release.part-no-version-iri"));
    // The abstract says three classes; two are declared.
    assert!(rules.contains(&"text.count-mismatch"));
    // Concepts and shapes have no rdfs:isDefinedBy.
    assert!(rules.contains(&"term.no-is-defined-by"));

    assert_eq!(
        report.summary.errors, 0,
        "the fixture has no error-level defects"
    );
}

#[test]
fn rule_selection_and_suppression() {
    let (release, store) = mini();
    let only_prefix = check::run(
        &release,
        &store,
        &check::Options {
            select: vec!["release.prefix".to_owned()],
            ..Default::default()
        },
    );
    assert!(
        only_prefix
            .findings
            .iter()
            .all(|f| f.rule.starts_with("release.prefix"))
    );

    let without_release = check::run(
        &release,
        &store,
        &check::Options {
            ignore: vec!["release.".to_owned()],
            ..Default::default()
        },
    );
    assert!(
        without_release
            .findings
            .iter()
            .all(|f| !f.rule.starts_with("release."))
    );
}

#[test]
fn the_model_is_deterministic() {
    let (a, _) = mini();
    let (b, _) = mini();
    let ja = serde_json::to_string(&a).unwrap();
    let jb = serde_json::to_string(&b).unwrap();
    assert_eq!(ja, jb, "two builds of the same inputs are identical");
}
