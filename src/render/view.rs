//! The view model: what a template sees.
//!
//! Templates never resolve an IRI, pick a language or compute a URL. Every
//! reference arrives as a `Ref` that already carries a label, a CURIE and,
//! when the release publishes it, a link. That is what lets a theme be a
//! directory of files a curator can edit, rather than code
//! (`docs/theming.md`, "Template context: `page`").
//!
//! Everything here is `Serialize`, sorted, and free of parse-order artefacts,
//! so the same inputs render the same bytes.

use super::{Ctx, humanise};
use crate::model::{Document, LangString, Term, TermKind};
use crate::site::{NamespacePlan, Rep};
use anyhow::Result;
use serde::Serialize;

/// A reference to a term, resolved for display.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Ref {
    pub iri: String,
    /// The label when the release knows the term, else the CURIE, else the IRI.
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curie: Option<String>,
    /// Where this term is published, when it is published here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The anchor on its document page, for terms this release documents but
    /// does not give a page of their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    pub foreign: bool,
}

/// A literal with its language and the predicate it came from, so a page can
/// say where a fact is from.
#[derive(Debug, Clone, Serialize)]
pub struct Value {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// The predicate IRI.
    pub source: String,
    /// The predicate as a CURIE, for display.
    pub source_label: String,
}

/// One representation of a page, for the head links and the footer list.
#[derive(Debug, Clone, Serialize)]
pub struct Sibling {
    pub rel: &'static str,
    pub media_type: &'static str,
    pub label: &'static str,
    pub url: String,
}

/// A node of a hierarchy rendered as nested lists, which stays accessible and
/// readable by an agent without a diagram.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub term: Ref,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PropertyView {
    pub domain: Vec<Ref>,
    pub range: Vec<Ref>,
    pub domain_includes: Vec<Ref>,
    pub range_includes: Vec<Ref>,
    pub inverse_of: Vec<Ref>,
    pub characteristics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConceptView {
    pub in_scheme: Vec<Ref>,
    pub top_concept_of: Vec<Ref>,
    pub broader: Vec<Ref>,
    pub narrower: Vec<Ref>,
    pub related: Vec<Ref>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MappingView {
    pub relation: String,
    pub relation_label: String,
    pub target: Ref,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatementView {
    pub predicate: String,
    pub predicate_label: String,
    pub object: String,
    pub object_is_iri: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

/// One row of a record template: what a shape says about one property.
///
/// A field is what a SHACL property shape means to someone filling in a
/// record. The vocabulary says a property exists and what its range is; the
/// shape says whether it is required, how many values it takes, what it is
/// called in a record, and which scheme its values come from. Both are true
/// and neither implies the other, so the page shows them side by side rather
/// than merging them.
/// A term that used to be here.
///
/// It carries the anchor it had, so a link into the previous release lands on
/// the note explaining where it went rather than on nothing. The term
/// has no page any more, so this is the only place that fragment can be
/// answered.
#[derive(Debug, Clone, Serialize)]
pub struct RemovedView {
    pub label: String,
    pub iri: String,
    pub anchor: String,
    pub detail: String,
}

/// One archived release, as a page needs it.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotView {
    pub segment: String,
    pub url: String,
    /// Where the version string was read from, said out loud so that a date
    /// is not mistaken for a release number.
    pub source: String,
    /// True when `owl:versionIRI` names exactly this snapshot.
    pub is_version_iri: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldView {
    /// `sh:name`, else the local name of the path.
    pub name: String,
    /// The property term, when the path is a plain predicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property: Option<Ref>,
    /// The path as text, for an inverse or a complex path.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
    pub required: bool,
    /// Datatypes, classes and node kinds as one readable phrase.
    pub value_type: String,
    /// Concept schemes the values must belong to.
    pub in_scheme: Vec<Ref>,
    /// The literal alternatives of an `sh:in`.
    pub values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The shape that imposes this, so a reader can tell OWL from SHACL.
    pub shape: Ref,
    pub deactivated: bool,
}

/// A node shape as a table of fields.
#[derive(Debug, Clone, Serialize)]
pub struct ShapeView {
    pub shape: Ref,
    /// How the shape selects what it applies to, in words.
    pub targeting: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    pub fields: Vec<FieldView>,
    pub required_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TermView {
    pub iri: String,
    pub local_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curie: Option<String>,
    pub anchor: String,
    /// Human wording, for example "object property".
    pub kind: String,
    /// Machine token, for a CSS class or a data attribute.
    pub kind_id: String,
    /// The heading a term reference groups this kind under.
    pub section: String,
    pub label: String,
    pub labels: Vec<Value>,
    pub alt_labels: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<Value>,
    pub definitions: Vec<Value>,
    pub comments: Vec<Value>,
    pub notes: Vec<Value>,
    pub examples: Vec<Value>,
    pub see_also: Vec<String>,
    pub super_terms: Vec<Ref>,
    pub sub_terms: Vec<Ref>,
    pub equivalent: Vec<Ref>,
    pub disjoint_with: Vec<Ref>,
    pub mappings: Vec<MappingView>,
    pub property: PropertyView,
    pub concept: ConceptView,
    pub deprecated: bool,
    pub replaced_by: Vec<Ref>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defined_in: Option<Ref>,
    /// Statements with no field of their own, so nothing is silently lost.
    pub residue: Vec<StatementView>,
    /// The term's own Turtle, ready to put in a code block.
    pub turtle: String,
    /// The term's identity: what `rel="canonical"` and `rel="cite-as"` say,
    /// and the same string under every `link_style`.
    pub url: String,
    /// Where a link to this term should send a browser, which is `url`
    /// unless `link_style = "file"`. A listing links here; a page's own
    /// canonical says `url`. They are the same field's two jobs, which is
    /// why they are now two fields.
    pub doc_url: String,
    pub siblings: Vec<Sibling>,
    /// For a class: the shapes that describe its instances, as record
    /// templates. For a node shape: its own fields.
    pub shapes: Vec<ShapeView>,
    /// For a property: what shapes say about it, each naming its source.
    pub constraints: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// A group of terms under one heading, in the order a page lists them.
#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub terms: Vec<TermView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentView {
    pub iri: String,
    pub kind: String,
    pub profile: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    pub abstract_paragraphs: Vec<String>,
    /// `rdfs:comment` on the document node, which application profiles use for
    /// a status banner rather than a description.
    pub comment: Vec<Value>,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_license: Option<String>,
    pub creators: Vec<AgentView>,
    pub publishers: Vec<AgentView>,
    pub contributors: Vec<AgentView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,
    pub see_also: Vec<String>,
    pub has_part: Vec<Ref>,
    /// Releases archived under this namespace, newest segment last. A
    /// citation of a version IRI resolves to one of these, so the page has to
    /// say they exist.
    pub versions: Vec<SnapshotView>,
    /// The URL of the version-link RDF, when there is any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions_url: Option<String>,
    /// The changelog, when this build was given a previous release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes_url: Option<String>,
    /// The PDF, when the build was asked for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
    /// How many changes it records, and how many break something.
    pub change_count: usize,
    pub breaking_count: usize,
    /// Terms this release no longer has.
    pub removed: Vec<RemovedView>,
    /// Term groups, one per kind present, in a stable order.
    pub sections: Vec<Section>,
    /// Terms reused from other vocabularies, documented but not published here.
    pub reused: Vec<TermView>,
    /// Nested-list hierarchy: class tree for an ontology, concept tree for a
    /// scheme. Empty when the document has no hierarchy worth showing.
    pub hierarchy: Vec<TreeNode>,
    pub term_count: usize,
    pub url: String,
    pub llms_url: String,
    pub siblings: Vec<Sibling>,
}

/// A namespace as a page lists it.
#[derive(Debug, Clone, Serialize)]
pub struct NamespaceView {
    pub iri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    pub url: String,
    pub llms_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub term_count: usize,
}

/// Site-wide values every page needs.
#[derive(Debug, Clone, Serialize)]
pub struct SiteView {
    pub base_url: String,
    pub lang: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub generator: String,
    pub generator_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_license: Option<String>,
    /// Whether the layout should include the colour-scheme control
    /// (`site.theme_switch`). A theme that supplies its own `base` decides
    /// for itself by including the partials, or not.
    pub theme_switch: bool,
    /// The human name of `doc_license`, because a link needs text.
    ///
    /// `base.html.jinja` has referenced `site.doc_license_label` since it was
    /// written and nothing ever supplied it, so minijinja rendered the
    /// undefined value as the empty string and every page carrying a
    /// documentation licence got `<a href="...."></a>`: an empty link, WCAG
    /// 2.4.4, which the auditor catches and the build refuses on. Setting
    /// `site.doc_license` -- a documented key -- made the site unbuildable,
    /// and no test noticed because the refusal is in `cli`, not here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_license_label: Option<String>,
    pub namespaces: Vec<NamespaceView>,
    pub prefixes: Vec<(String, String)>,
    pub llms_url: String,
    pub terms_url: String,
    pub manifest_url: String,
    pub release_ttl_url: String,
    pub term_count: usize,
    pub document_count: usize,
}

fn value_of(ctx: &Ctx<'_>, l: &LangString) -> Value {
    Value {
        value: l.value.clone(),
        lang: l.lang.clone(),
        source: l.source.clone(),
        source_label: ctx.short(&l.source),
    }
}

fn values_of(ctx: &Ctx<'_>, list: &[LangString]) -> Vec<Value> {
    list.iter().map(|l| value_of(ctx, l)).collect()
}

/// Resolve an IRI into something a template can render without lookups.
pub fn reference(ctx: &Ctx<'_>, iri: &str) -> Ref {
    let lang = ctx.lang();
    match ctx.release.term(iri) {
        Some(t) => {
            let url = if t.foreign {
                None
            } else {
                // A cross-reference is navigation, so it follows
                // `link_style`; `Ref` carries the IRI beside it for anything
                // that needs identity.
                ctx.namespace_of(t)
                    .map(|ns| ctx.plan.term_doc_url(ns, &t.local_name))
            };
            Ref {
                iri: iri.to_owned(),
                label: t.display(lang).to_owned(),
                curie: t.curie.clone(),
                url,
                anchor: Some(t.anchor.clone()),
                foreign: t.foreign,
            }
        }
        None => {
            // Not a term of this release: a document, or an external IRI.
            let document = ctx.release.document(iri);
            let url = document.and_then(|d| {
                ctx.namespace_of_document(d)
                    .map(|ns| ctx.plan.document_url(ns, Rep::Html))
            });
            let curie = crate::model::curie(iri, &ctx.release.prefixes);
            Ref {
                label: document
                    .map(|d| d.display(lang).to_owned())
                    .or_else(|| curie.clone())
                    .unwrap_or_else(|| iri.to_owned()),
                iri: iri.to_owned(),
                curie,
                url,
                anchor: None,
                foreign: document.is_none(),
            }
        }
    }
}

fn references(ctx: &Ctx<'_>, iris: &[String]) -> Vec<Ref> {
    iris.iter().map(|i| reference(ctx, i)).collect()
}

fn siblings(ctx: &Ctx<'_>, ns: &NamespacePlan, local: Option<&str>) -> Vec<Sibling> {
    let url = |rep: Rep| match local {
        Some(l) => ctx.plan.term_url(ns, l, rep),
        None => ctx.plan.document_url(ns, rep),
    };
    Rep::produced()
        .into_iter()
        .filter(|r| *r != Rep::Html)
        .map(|r| Sibling {
            rel: "alternate",
            media_type: r.media_type(),
            label: match r {
                Rep::Markdown => "Markdown",
                Rep::Turtle => "Turtle",
                Rep::JsonLd => "JSON-LD",
                Rep::Html => "HTML",
            },
            url: url(r),
        })
        .collect()
}

/// Datatypes, classes and node kind as one phrase a reader can act on.
fn value_type(ctx: &Ctx<'_>, p: &crate::shape::PropertyShape) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(p.datatypes.iter().map(|d| ctx.short(d)));
    parts.extend(p.classes.iter().map(|c| ctx.short(c)));
    if parts.is_empty()
        && let Some(kind) = &p.node_kind
    {
        // `sh:IRI` says the value is a reference without saying to what,
        // which is worth stating rather than leaving the column empty.
        parts.push(match crate::vocab::local_name(kind) {
            "IRI" => "IRI".to_owned(),
            "Literal" => "literal".to_owned(),
            "BlankNode" => "blank node".to_owned(),
            "IRIOrLiteral" => "IRI or literal".to_owned(),
            other => other.to_owned(),
        });
    }
    parts.join(" or ")
}

fn field(
    ctx: &Ctx<'_>,
    shape: &crate::shape::NodeShape,
    p: &crate::shape::PropertyShape,
) -> FieldView {
    let path_text = match &p.path {
        Some(crate::shape::Path::Predicate { iri }) => ctx.short(iri),
        Some(crate::shape::Path::Inverse { iri }) => format!("inverse of {}", ctx.short(iri)),
        Some(crate::shape::Path::Complex { description }) => description.clone(),
        None => String::new(),
    };
    let property = p
        .path
        .as_ref()
        .and_then(crate::shape::Path::predicate)
        .map(|iri| reference(ctx, iri));
    let name = p
        .name
        .clone()
        .or_else(|| property.as_ref().map(|r| r.label.clone()))
        .unwrap_or_else(|| path_text.clone());

    FieldView {
        name,
        property,
        path: path_text,
        cardinality: p.cardinality(),
        required: p.required(),
        value_type: value_type(ctx, p),
        in_scheme: references(ctx, &p.in_scheme),
        values: p.values.clone(),
        pattern: p.pattern.clone(),
        description: p.description.clone(),
        shape: reference(ctx, &shape.iri),
        deactivated: p.deactivated,
    }
}

/// How a shape says what it applies to, in words rather than in SHACL.
fn targeting(ctx: &Ctx<'_>, shape: &crate::shape::NodeShape) -> String {
    let mut parts = Vec::new();
    if !shape.target_classes.is_empty() {
        let names: Vec<String> = shape.target_classes.iter().map(|c| ctx.short(c)).collect();
        parts.push(format!("instances of {}", names.join(", ")));
    }
    for property in &shape.target_subjects_of {
        // Naming the property matters: this is why the shape reaches a class
        // it never mentions.
        parts.push(format!("anything with a {} statement", ctx.short(property)));
    }
    for property in &shape.target_objects_of {
        parts.push(format!("the values of {}", ctx.short(property)));
    }
    if !shape.target_nodes.is_empty() {
        parts.push(format!("{} named nodes", shape.target_nodes.len()));
    }
    if parts.is_empty() {
        return "nothing on its own; it is used by another shape".to_owned();
    }
    parts.join("; ")
}

fn shape_view(ctx: &Ctx<'_>, shape: &crate::shape::NodeShape) -> ShapeView {
    let fields: Vec<FieldView> = shape
        .properties
        .iter()
        .map(|p| field(ctx, shape, p))
        .collect();
    ShapeView {
        shape: reference(ctx, &shape.iri),
        targeting: targeting(ctx, shape),
        closed: shape.closed,
        required_count: fields.iter().filter(|f| f.required).count(),
        fields,
    }
}

/// The shapes a term should show: its own if it is a node shape, and the ones
/// that describe its instances if it is a class.
fn shapes_for(ctx: &Ctx<'_>, t: &Term) -> Vec<ShapeView> {
    let shapes = &ctx.release.shapes;
    if t.kind == crate::model::TermKind::NodeShape
        && let Some(own) = shapes.shape(&t.iri)
    {
        return vec![shape_view(ctx, own)];
    }
    if t.kind == crate::model::TermKind::Class {
        return shapes
            .for_class(&t.iri)
            .into_iter()
            .map(|s| shape_view(ctx, s))
            .collect();
    }
    Vec::new()
}

/// Build the view of one term.
pub fn term(ctx: &Ctx<'_>, ns: &NamespacePlan, t: &Term) -> Result<TermView> {
    let lang = ctx.lang();
    let residue = t
        .residue
        .iter()
        .map(|s| {
            let (object, object_is_iri, lang) = match &s.object {
                crate::model::Node::Iri { iri } => (iri.clone(), true, None),
                crate::model::Node::Literal { value, lang, .. } => {
                    (value.clone(), false, lang.clone())
                }
                crate::model::Node::Blank { id } => (format!("_:{id}"), false, None),
            };
            StatementView {
                predicate: s.predicate.clone(),
                predicate_label: ctx.short(&s.predicate),
                object,
                object_is_iri,
                lang,
            }
        })
        .collect();

    Ok(TermView {
        iri: t.iri.clone(),
        local_name: t.local_name.clone(),
        curie: t.curie.clone(),
        anchor: t.anchor.clone(),
        kind: t.kind.label().to_owned(),
        kind_id: format!("{:?}", t.kind).to_lowercase(),
        section: t.kind.section().to_owned(),
        label: t.display(lang).to_owned(),
        labels: values_of(ctx, &t.labels),
        alt_labels: values_of(ctx, &t.alt_labels),
        definition: t.definition(lang).map(|d| value_of(ctx, d)),
        definitions: values_of(ctx, &t.definitions),
        comments: values_of(ctx, &t.comments),
        notes: values_of(ctx, &t.notes),
        examples: values_of(ctx, &t.examples),
        see_also: t.see_also.clone(),
        super_terms: references(ctx, &t.super_terms),
        sub_terms: references(ctx, &t.sub_terms),
        equivalent: references(ctx, &t.equivalent),
        disjoint_with: references(ctx, &t.disjoint_with),
        mappings: t
            .mappings
            .iter()
            .map(|m| MappingView {
                relation: m.relation.clone(),
                relation_label: humanise(&m.relation),
                target: reference(ctx, &m.iri),
            })
            .collect(),
        property: PropertyView {
            domain: references(ctx, &t.property.domain),
            range: references(ctx, &t.property.range),
            domain_includes: references(ctx, &t.property.domain_includes),
            range_includes: references(ctx, &t.property.range_includes),
            inverse_of: references(ctx, &t.property.inverse_of),
            characteristics: t.property.characteristics.clone(),
        },
        concept: ConceptView {
            in_scheme: references(ctx, &t.concept.in_scheme),
            top_concept_of: references(ctx, &t.concept.top_concept_of),
            broader: references(ctx, &t.concept.broader),
            narrower: references(ctx, &t.concept.narrower),
            related: references(ctx, &t.concept.related),
            notation: t.concept.notation.clone(),
        },
        deprecated: t.deprecated,
        replaced_by: references(ctx, &t.replaced_by),
        status: t.status.as_deref().map(humanise),
        defined_in: t.defined_in.as_deref().map(|d| reference(ctx, d)),
        residue,
        turtle: super::rdf::term(ctx.store, &t.iri, &ctx.release.prefixes)?,
        url: ctx.plan.term_url(ns, &t.local_name, Rep::Html),
        doc_url: ctx.plan.term_doc_url(ns, &t.local_name),
        siblings: siblings(ctx, ns, Some(&t.local_name)),
        shapes: shapes_for(ctx, t),
        constraints: ctx
            .release
            .shapes
            .for_property(&t.iri)
            .into_iter()
            .map(|(shape, p)| field(ctx, shape, p))
            .collect(),
    })
}

/// Roots and children of the hierarchy a document should show.
fn hierarchy(ctx: &Ctx<'_>, doc: &Document) -> Vec<TreeNode> {
    let members: Vec<&Term> = doc
        .terms
        .iter()
        .filter_map(|iri| ctx.release.term(iri))
        .collect();
    if members.is_empty() {
        return Vec::new();
    }
    let inside = |iri: &String| members.iter().any(|m| &m.iri == iri);

    let concepts: Vec<&&Term> = members
        .iter()
        .filter(|t| t.kind == TermKind::Concept)
        .collect();
    let classes: Vec<&&Term> = members
        .iter()
        .filter(|t| t.kind == TermKind::Class)
        .collect();

    type IsRoot<'a> = Box<dyn Fn(&Term) -> bool + 'a>;
    let (pool, is_root): (Vec<&&Term>, IsRoot<'_>) = if !concepts.is_empty() {
        (
            concepts,
            Box::new(move |t: &Term| {
                !t.concept.top_concept_of.is_empty() || t.concept.broader.is_empty()
            }),
        )
    } else if !classes.is_empty() {
        (classes, Box::new(move |t: &Term| t.super_terms.is_empty()))
    } else {
        return Vec::new();
    };

    // A root is one with no parent inside this document.
    let roots: Vec<&&Term> = pool
        .iter()
        .filter(|t| {
            let parents: &Vec<String> = if t.kind == TermKind::Concept {
                &t.concept.broader
            } else {
                &t.super_terms
            };
            !parents.iter().any(inside) || is_root(t)
        })
        .copied()
        .collect();

    fn node(ctx: &Ctx<'_>, t: &Term, pool: &[&&Term], depth: usize) -> TreeNode {
        let children = if depth > 8 {
            Vec::new()
        } else {
            let mut kids: Vec<TreeNode> = pool
                .iter()
                .filter(|c| {
                    let parents: &Vec<String> = if c.kind == TermKind::Concept {
                        &c.concept.broader
                    } else {
                        &c.super_terms
                    };
                    parents.contains(&t.iri)
                })
                .map(|c| node(ctx, c, pool, depth + 1))
                .collect();
            kids.sort_by(|a, b| a.term.label.cmp(&b.term.label));
            kids
        };
        TreeNode {
            term: reference(ctx, &t.iri),
            children,
        }
    }

    let mut tree: Vec<TreeNode> = roots.iter().map(|t| node(ctx, t, &pool, 0)).collect();
    tree.sort_by(|a, b| a.term.label.cmp(&b.term.label));
    tree
}

/// Build the view of a document page.
pub fn document(ctx: &Ctx<'_>, ns: &NamespacePlan, doc: &Document) -> Result<DocumentView> {
    let lang = ctx.lang();

    let mut sections: Vec<Section> = Vec::new();
    let mut kinds: Vec<TermKind> = doc
        .terms
        .iter()
        .filter_map(|iri| ctx.release.term(iri).map(|t| t.kind))
        .collect();
    kinds.sort();
    kinds.dedup();
    for kind in kinds {
        let mut terms = Vec::new();
        for iri in &doc.terms {
            let Some(t) = ctx.release.term(iri) else {
                continue;
            };
            if t.kind != kind {
                continue;
            }
            let home = ctx.namespace_of(t).unwrap_or(ns);
            terms.push(term(ctx, home, t)?);
        }
        terms.sort_by_key(|t| t.label.to_lowercase());
        sections.push(Section {
            id: kind.section().to_lowercase().replace(' ', "-"),
            title: kind.section().to_owned(),
            terms,
        });
    }

    let mut reused = Vec::new();
    for iri in &doc.foreign_terms {
        if let Some(t) = ctx.release.term(iri) {
            reused.push(term(ctx, ns, t)?);
        }
    }
    reused.sort_by(|a, b| a.iri.cmp(&b.iri));

    let abstract_paragraphs = doc
        .header
        .abstract_
        .iter()
        .flat_map(|a| {
            a.value
                .split("\n\n")
                .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();

    let term_count = doc.terms.len();
    Ok(DocumentView {
        iri: doc.iri.clone(),
        kind: format!("{:?}", doc.kind).to_lowercase(),
        profile: doc.profile.clone(),
        title: doc.display(lang).to_owned(),
        description: doc.description(lang).map(|d| value_of(ctx, d)),
        abstract_paragraphs,
        comment: values_of(ctx, &doc.header.comment),
        namespace: ns.iri.clone(),
        prefix: ns.prefix.clone(),
        version: doc.header.version_info.clone(),
        version_iri: doc.header.version_iri.clone(),
        status: doc.header.status.as_deref().map(humanise),
        status_iri: doc.header.status.clone(),
        license: doc.header.license.clone(),
        doc_license: ctx.config.site.doc_license.clone(),
        creators: doc.header.creators.iter().map(agent).collect(),
        publishers: doc.header.publishers.iter().map(agent).collect(),
        contributors: doc.header.contributors.iter().map(agent).collect(),
        created: doc.header.created.clone(),
        modified: doc.header.modified.clone(),
        issued: doc.header.issued.clone(),
        citation: doc.header.citation.clone(),
        see_also: doc.header.see_also.clone(),
        has_part: references(ctx, &doc.header.has_part),
        versions: {
            let mut v: Vec<SnapshotView> = ctx
                .snapshots(ns)
                .into_iter()
                .map(|s| SnapshotView {
                    segment: s.segment,
                    url: s.url,
                    source: s.source.label().to_owned(),
                    is_version_iri: s.version_iri_resolves,
                })
                .collect();
            v.sort_by(|a, b| a.segment.cmp(&b.segment));
            v
        },
        versions_url: (!ctx.snapshots(ns).is_empty())
            .then(|| format!("{}{}versions.ttl", ctx.plan.base_url, ns.mount)),
        pdf_url: ctx
            .config
            .site
            .pdf
            .then(|| format!("{}{}{}.pdf", ctx.plan.base_url, ns.mount, ns.stem)),
        changes_url: ctx
            .changes
            .filter(|d| {
                d.changes
                    .iter()
                    .any(|c| c.namespace.as_deref() == Some(ns.iri.as_str()))
            })
            .map(|_| format!("{}{}changes.md", ctx.plan.base_url, ns.mount)),
        change_count: ctx
            .changes
            .map(|d| {
                d.changes
                    .iter()
                    .filter(|c| c.namespace.as_deref() == Some(ns.iri.as_str()))
                    .count()
            })
            .unwrap_or(0),
        breaking_count: ctx
            .changes
            .map(|d| {
                d.changes
                    .iter()
                    .filter(|c| {
                        c.namespace.as_deref() == Some(ns.iri.as_str())
                            && c.severity == crate::diff::Severity::Breaking
                    })
                    .count()
            })
            .unwrap_or(0),
        removed: ctx
            .changes
            .map(|d| {
                d.changes
                    .iter()
                    .filter(|c| {
                        c.rule == "term.removed" && c.namespace.as_deref() == Some(ns.iri.as_str())
                    })
                    .map(|c| RemovedView {
                        label: c.label.clone().unwrap_or_default(),
                        iri: c.iri.clone().unwrap_or_default(),
                        anchor: c.anchor.clone().unwrap_or_default(),
                        detail: c.detail.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        sections,
        reused,
        hierarchy: hierarchy(ctx, doc),
        term_count,
        url: ctx.plan.document_url(ns, Rep::Html),
        llms_url: ctx.plan.llms_url(ns),
        siblings: siblings(ctx, ns, None),
    })
}

fn agent(a: &crate::model::Agent) -> AgentView {
    AgentView {
        name: a.name.clone(),
        iri: a.iri.clone(),
        kind: a
            .kind
            .as_deref()
            .map(crate::vocab::local_name)
            .map(str::to_owned),
    }
}

/// Build the site-wide view.
pub fn site(ctx: &Ctx<'_>) -> SiteView {
    let lang = ctx.lang();
    let root = ctx.release.root_document();
    SiteView {
        base_url: ctx.plan.base_url.clone(),
        lang: lang.to_owned(),
        title: ctx
            .config
            .site
            .title
            .clone()
            .or_else(|| root.map(|d| d.display(lang).to_owned()))
            .unwrap_or_else(|| "Vocabulary".to_owned()),
        description: root
            .and_then(|d| d.description(lang))
            .map(|d| d.value.split_whitespace().collect::<Vec<_>>().join(" ")),
        generator: env!("CARGO_PKG_NAME").to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        doc_license: ctx.config.site.doc_license.clone(),
        theme_switch: ctx.config.site.theme_switch,
        doc_license_label: ctx
            .config
            .site
            .doc_license
            .as_deref()
            .map(super::html::licence_label),
        namespaces: ctx
            .plan
            .namespaces
            .iter()
            .map(|ns| {
                let doc = ns
                    .document
                    .as_deref()
                    .and_then(|iri| ctx.release.document(iri));
                NamespaceView {
                    iri: ns.iri.clone(),
                    prefix: ns.prefix.clone(),
                    url: ctx.plan.document_url(ns, Rep::Html),
                    llms_url: ctx.plan.llms_url(ns),
                    title: doc.map(|d| d.display(lang).to_owned()),
                    term_count: doc.map(|d| d.terms.len()).unwrap_or(0),
                }
            })
            .collect(),
        prefixes: ctx
            .release
            .prefixes
            .iter()
            .map(|(p, n)| (p.clone(), n.clone()))
            .collect(),
        llms_url: format!("{}llms.txt", ctx.plan.base_url),
        terms_url: format!("{}terms.json", ctx.plan.base_url),
        manifest_url: format!("{}manifest.json", ctx.plan.base_url),
        release_ttl_url: format!("{}release.ttl", ctx.plan.base_url),
        term_count: ctx.release.stats.terms_local,
        document_count: ctx.release.stats.documents,
    }
}
