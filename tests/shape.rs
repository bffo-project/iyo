//! Tests for the SHACL join: shapes reaching the terms they constrain.
//!
//! The join is what makes a SHACL file documentation rather than a
//! validator's input. Each test here stands for a route this module names, and
//! the third is the one that carries real weight: BFFO targets subjects of a
//! property rather than a class, so without following `rdfs:domain` the class
//! page would show nothing at all.

use camino::Utf8PathBuf;
use iyo::config::Config;
use iyo::render::Ctx;
use iyo::shape::Path;
use iyo::site::Plan;
use iyo::{build, load, model::Release, profile, render};
use std::collections::BTreeMap;

fn release() -> (Release, iyo::load::Store) {
    let base = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
    let store = load::load(&paths).unwrap();
    let registry = profile::Registry::built_in().unwrap();
    let release = build::build(&store, &registry).unwrap();
    (release, store)
}

fn rendered() -> BTreeMap<String, String> {
    let (release, store) = release();
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

#[test]
fn a_shape_is_read_into_fields_rather_than_left_as_blank_nodes() {
    let (release, _) = release();
    let shape = release
        .shapes
        .shape("https://example.org/vocab/WidgetShape")
        .expect("the widget shape");
    assert_eq!(shape.properties.len(), 2);

    let category = shape
        .properties
        .iter()
        .find(|p| p.name.as_deref() == Some("category"))
        .expect("the category field");
    assert_eq!(
        category.path.as_ref().and_then(Path::predicate),
        Some("https://example.org/vocab/category")
    );
    assert_eq!(category.cardinality().as_deref(), Some("1..1"));
    assert!(category.required());
    // The scheme is two blank nodes deep, in `sh:node [ sh:property [ … ] ]`,
    // which is how a vocabulary says "values from this scheme".
    assert_eq!(
        category.in_scheme,
        vec!["https://example.org/vocabulary/category/".to_owned()]
    );
}

#[test]
fn a_shape_reaches_a_class_through_the_domain_of_the_property_it_targets() {
    let (release, _) = release();
    // The fixture shape targets subjects of `ex:serial`, never naming a
    // class. `ex:serial` has `rdfs:domain ex:Widget`, which is the only route
    // from the shape to the class page.
    let shape = release
        .shapes
        .shape("https://example.org/vocab/WidgetShape")
        .unwrap();
    assert!(
        shape.target_classes.is_empty(),
        "the fixture targets no class"
    );
    assert_eq!(
        shape.target_subjects_of,
        vec!["https://example.org/vocab/serial".to_owned()]
    );
    assert_eq!(
        shape.applies_to,
        vec!["https://example.org/vocab/Widget".to_owned()]
    );

    let for_class = release.shapes.for_class("https://example.org/vocab/Widget");
    assert_eq!(for_class.len(), 1);
    assert_eq!(for_class[0].iri, "https://example.org/vocab/WidgetShape");
}

#[test]
fn a_property_shape_reaches_its_property_through_sh_path() {
    let (release, _) = release();
    let found = release
        .shapes
        .for_property("https://example.org/vocab/category");
    assert_eq!(found.len(), 1);
    let (shape, property) = found[0];
    assert_eq!(shape.iri, "https://example.org/vocab/WidgetShape");
    assert_eq!(property.name.as_deref(), Some("category"));
}

#[test]
fn the_class_page_shows_a_record_template_naming_its_shape() {
    let files = rendered();
    let page = files
        .get("vocab/Widget.html")
        .expect("the widget class page");
    assert!(page.contains("Record template"));
    assert!(
        page.contains("anything with a ex:serial statement"),
        "the page does not say how the shape selects what it applies to"
    );
    // The property is linked from the template, not just named. The href is
    // relative, so `vocab/Widget.html` reaches its sibling through `../vocab/`.
    assert!(page.contains(r#"href="../vocab/category""#));

    let md = files.get("vocab/Widget.md").expect("the widget markdown");
    assert!(md.contains("## Record template from ex:WidgetShape"));
    assert!(md.contains("| Field | Property | Values | Count | Description |"));
}

#[test]
fn a_property_page_keeps_owl_and_shacl_apart() {
    let files = rendered();
    let md = files
        .get("vocab/category.md")
        .expect("the category markdown");
    // Both claims are present and each says where it came from, rather than
    // one silently overriding the other.
    assert!(md.contains("- Range: skos:Concept"));
    assert!(md.contains("(source: SHACL ex:WidgetShape)"));
    assert!(md.contains("- Cardinality: 1..1, required (source: SHACL ex:WidgetShape)"));

    let page = files.get("vocab/category.html").unwrap();
    assert!(page.contains("Constraints"));
}

#[test]
fn shape_structure_does_not_reach_the_residue() {
    let files = rendered();
    let page = files.get("vocab/WidgetShape.html").unwrap();
    // Before the join, a shape page listed one `sh:property _:bN` row per
    // field under "other statements" and said nothing about any of them.
    // The verbatim Turtle block still carries them, and should: nothing is
    // dropped, it is only no longer the only place the reader can look.
    let residue = page
        .split_once("id=\"iyo-other\"")
        .map(|(_, rest)| rest.split("</section>").next().unwrap_or(""))
        .unwrap_or("");
    assert!(
        !residue.contains("sh:property"),
        "the shape's structure is still being dumped as raw statements"
    );
    assert!(page.contains("Constraints"));
    assert!(page.contains("Widget Shape"));
    // The Turtle is still complete.
    assert!(page.contains("sh:property"));
}
