//! Tests for the JSON-LD output: what the context says, how values are
//! written, and that the triples survive the trip.
//!
//! The build already refuses to write a JSON-LD file that does not parse back
//! to the triples that went in, so these tests are about the *shape* of what
//! it writes: whether the file is small, readable, and usable by a reader who
//! has no JSON-LD processor at all.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::site::Plan;
use iyo::{build, load, profile, render};
use serde_json::Value;
use std::collections::BTreeMap;

fn built() -> BTreeMap<String, String> {
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
}

fn json(files: &BTreeMap<String, String>, path: &str) -> Value {
    serde_json::from_str(
        files
            .get(path)
            .unwrap_or_else(|| panic!("{path} was not written")),
    )
    .expect("valid JSON")
}

#[test]
fn a_context_document_carries_nothing_but_its_context() {
    let files = built();
    for path in ["context.jsonld", "vocab/context.jsonld"] {
        let doc = json(&files, path);
        let object = doc.as_object().expect("an object");
        // Written as a bare mapping instead, every prefix declaration becomes
        // a triple when the file is dereferenced.
        assert_eq!(
            object.keys().collect::<Vec<_>>(),
            vec!["@context"],
            "{path} has a key other than @context"
        );
        assert!(object["@context"].is_object());
    }
}

#[test]
fn a_published_context_keys_terms_by_their_local_name() {
    let files = built();
    let context = json(&files, "vocab/context.jsonld");
    let map = context["@context"].as_object().unwrap();

    // A prefix is a plain string; a term is a string or an object with @id.
    assert_eq!(map["ex"], "https://example.org/vocab/");
    // An object property says its values are IRIs, so a reader knows that
    // `"category": "ex-cat:round"` is a reference and not a label.
    assert_eq!(map["category"]["@id"], "ex:category");
    assert_eq!(map["category"]["@type"], "@id");
    // A datatype property carries its datatype instead.
    assert_eq!(map["serial"]["@id"], "ex:serial");
    assert_eq!(map["serial"]["@type"], "xsd:string");
}

#[test]
fn a_term_file_inlines_only_the_predicates_it_writes() {
    let files = built();
    let shape = json(&files, "vocab/WidgetShape.jsonld");
    let context = shape["@context"].as_object().unwrap();

    // The shape mentions `ex:category` as the object of `sh:path`, where the
    // context key is never consulted. Carrying an entry for it would drag
    // most of the vocabulary into every shape file.
    assert!(
        !context.contains_key("category"),
        "an entry was inlined for a term used only as a value"
    );
    // The prefixes it does need are there.
    assert_eq!(context["sh"], "http://www.w3.org/ns/shacl#");
    assert!(context.contains_key("ex"));
}

#[test]
fn a_reader_with_no_processor_still_gets_usable_values() {
    let files = built();
    let shape = json(&files, "vocab/WidgetShape.jsonld");

    // A boolean is a JSON boolean and an integer is a JSON number, because
    // both expand back to exactly the datatype they came from.
    assert_eq!(shape["sh:closed"], Value::Bool(false));
    let properties = shape["sh:property"].as_array().expect("an array");
    let counts: Vec<&Value> = properties.iter().map(|p| &p["sh:maxCount"]).collect();
    assert!(
        counts.iter().all(|c| c.is_number()),
        "an integer was written as a string: {counts:?}"
    );

    // The identity is the absolute IRI, not a CURIE the reader must expand.
    assert_eq!(shape["@id"], "https://example.org/vocab/WidgetShape");
}

#[test]
fn a_blank_node_is_nested_where_it_is_used() {
    let files = built();
    let shape = json(&files, "vocab/WidgetShape.jsonld");
    let properties = shape["sh:property"].as_array().expect("an array");

    // A property shape is an anonymous node, so it belongs inside the shape
    // that owns it rather than beside it under a generated label.
    let category = properties
        .iter()
        .find(|p| p["sh:name"] == "category")
        .expect("the category property shape");
    assert!(category.get("@id").is_none(), "a blank node kept a label");
    // Nesting goes as deep as the data does.
    assert_eq!(
        category["sh:node"]["sh:property"]["sh:path"]["@id"],
        "skos:inScheme"
    );
}

#[test]
fn a_language_tagged_literal_keeps_its_tag_and_a_plain_one_stays_bare() {
    let files = built();
    let round = json(&files, "vocabulary/category/round.jsonld");
    assert_eq!(round["skos:prefLabel"]["@value"], "Round");
    assert_eq!(round["skos:prefLabel"]["@language"], "en");

    // An untagged string needs no wrapper, since nothing would re-read it.
    assert_eq!(round["skos:notation"], "round");

    let widget = json(&files, "vocab/Widget.jsonld");
    assert_eq!(widget["rdfs:label"], "Widget");
}

#[test]
fn every_generated_file_is_valid_json() {
    let files = built();
    let count = files.keys().filter(|p| p.ends_with(".jsonld")).count();
    assert!(count >= 10, "only {count} JSON-LD files were written");
    for (path, content) in &files {
        if path.ends_with(".jsonld") {
            serde_json::from_str::<Value>(content)
                .unwrap_or_else(|e| panic!("{path} is not valid JSON: {e}"));
        }
    }
}

/// The same release given twice is still one release. `iyo build a/ b/`
/// where both directories describe the same vocabulary used to fail with
/// "JSON-LD does not round-trip: wrote 197 triples, read back 182" and
/// nothing named as lost or invented, because nothing was: the verifier
/// compared multisets where RDF is a set.
#[test]
fn the_same_triple_from_two_files_round_trips_as_one() {
    let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = iyo::load::expand_inputs(
        &["testdata/mini".to_owned(), "testdata/previous".to_owned()],
        &root,
    )
    .expect("both fixtures");
    let store = iyo::load::load(&paths).expect("loading both");
    let registry = iyo::profile::Registry::built_in().expect("the built-in profiles");
    let release = iyo::build::build(&store, &registry).expect("building");
    let mut config = iyo::config::Config::implicit();
    config.site.base_url = "https://example.org/".to_owned();
    let plan = iyo::site::Plan::new(&release, &config);
    let ctx = iyo::render::Ctx {
        release: &release,
        store: &store,
        plan: &plan,
        config: &config,
        changes: None,
    };
    // `render` runs `jsonld::verify` on every JSON-LD file it writes, so a
    // regression here is this call returning Err.
    iyo::render::render(&ctx).expect("a release described twice still renders");
}
