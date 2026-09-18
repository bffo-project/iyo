//! SHACL shapes as a model, and the join from a shape to the terms it
//! constrains.
//!
//! A SHACL document is documentation whether or not it was written as such: it
//! is where the cardinality of a property, the vocabulary its values come
//! from, and the name a record uses for it are actually written down. Left as
//! RDF it says none of that on the page of the property it constrains. That
//! asks for the join in both directions:
//!
//! - a property shape reaches its property term through `sh:path`;
//! - a node shape reaches a class through `sh:targetClass`, through being a
//!   class itself, or, when it targets by `sh:targetSubjectsOf`, through the
//!   `rdfs:domain` of the property it targets.
//!
//! The third route is the one that matters in practice. BFFO deliberately
//! targets subjects of `bffo:formatType` rather than `bffo:Format`, so that
//! merely referring to a format does not demand a full record for it. Without
//! following `rdfs:domain`, the class the shape describes would show nothing.
//!
//! Nothing here validates. Running constraints is a job for a SHACL engine;
//! this module only reads what the shapes say.

use crate::load::Store;
use crate::vocab as v;
use oxrdf::Term;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// What a property shape constrains, when it is not a plain predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Path {
    /// `sh:path ex:name`, the only form that joins to a term page.
    Predicate { iri: String },
    /// `sh:path [ sh:inversePath ex:name ]`.
    Inverse { iri: String },
    /// A sequence, alternative or repetition, kept as text so that the page
    /// says something true rather than nothing.
    Complex { description: String },
}

impl Path {
    /// The property this path constrains directly, if any.
    pub fn predicate(&self) -> Option<&str> {
        match self {
            Path::Predicate { iri } => Some(iri),
            _ => None,
        }
    }
}

/// One `sh:property` constraint.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PropertyShape {
    pub path: Option<Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Every datatype the constraint allows, including the alternatives of an
    /// `sh:or`, sorted.
    pub datatypes: Vec<String>,
    pub classes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_count: Option<u64>,
    /// The literal values of an `sh:in` enumeration.
    pub values: Vec<String>,
    /// `sh:hasValue`.
    pub has_value: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// The concept scheme values must belong to, read from the common
    /// `sh:node [ sh:property [ sh:path skos:inScheme ; sh:hasValue S ] ]`
    /// idiom. This is how a vocabulary says "values from this scheme", and it
    /// is invisible unless the nesting is followed.
    pub in_scheme: Vec<String>,
    /// A named shape the value must also conform to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    pub deactivated: bool,
}

impl PropertyShape {
    /// `1..1`, `0..*`, and so on. `None` when the shape says nothing about
    /// how many values there may be, which is not the same as "any number".
    pub fn cardinality(&self) -> Option<String> {
        if self.min_count.is_none() && self.max_count.is_none() {
            return None;
        }
        let min = self.min_count.unwrap_or(0);
        let max = match self.max_count {
            Some(n) => n.to_string(),
            None => "*".to_owned(),
        };
        Some(format!("{min}..{max}"))
    }

    pub fn required(&self) -> bool {
        self.min_count.unwrap_or(0) >= 1
    }
}

/// One shape with targets: a `sh:NodeShape`, or anything carrying
/// `sh:property` or a target.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeShape {
    pub iri: String,
    pub target_classes: Vec<String>,
    pub target_subjects_of: Vec<String>,
    pub target_objects_of: Vec<String>,
    pub target_nodes: Vec<String>,
    /// The classes this shape describes, after following
    /// `sh:targetSubjectsOf` to the `rdfs:domain` of the targeted property.
    /// This is what a class page looks itself up in.
    pub applies_to: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    pub properties: Vec<PropertyShape>,
}

/// Every shape in a release, with the indexes the join needs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Shapes {
    pub node_shapes: Vec<NodeShape>,
}

/// Walk an RDF collection, stopping on a cycle rather than looping.
fn rdf_list(store: &Store, head: &str) -> Vec<Term> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut node = head.to_owned();
    while node != v::RDF_NIL && seen.insert(node.clone()) {
        if let Some(first) = store.objects(&node, v::RDF_FIRST).first() {
            out.push((*first).clone());
        }
        match store.objects(&node, v::RDF_REST).first() {
            Some(Term::NamedNode(n)) => node = n.as_str().to_owned(),
            Some(Term::BlankNode(b)) => node = format!("_:{}", b.as_str()),
            _ => break,
        }
    }
    out
}

fn node_key(term: &Term) -> Option<String> {
    match term {
        Term::NamedNode(n) => Some(n.as_str().to_owned()),
        Term::BlankNode(b) => Some(format!("_:{}", b.as_str())),
        Term::Literal(_) => None,
    }
}

fn iris(store: &Store, subject: &str, predicate: &str) -> Vec<String> {
    let mut out: Vec<String> = store
        .objects(subject, predicate)
        .into_iter()
        .filter_map(|t| match t {
            Term::NamedNode(n) => Some(n.as_str().to_owned()),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn literal(store: &Store, subject: &str, predicate: &str) -> Option<String> {
    store
        .objects(subject, predicate)
        .into_iter()
        .find_map(|t| match t {
            Term::Literal(l) => Some(l.value().to_owned()),
            _ => None,
        })
}

fn count(store: &Store, subject: &str, predicate: &str) -> Option<u64> {
    literal(store, subject, predicate).and_then(|s| s.parse().ok())
}

fn boolean(store: &Store, subject: &str, predicate: &str) -> Option<bool> {
    match literal(store, subject, predicate)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Read `sh:path`, which is an IRI in the simple case and a blank node
/// describing a path expression otherwise.
fn path(store: &Store, subject: &str) -> Option<Path> {
    match store.objects(subject, v::SH_PATH).first()? {
        Term::NamedNode(n) => Some(Path::Predicate {
            iri: n.as_str().to_owned(),
        }),
        Term::BlankNode(b) => {
            let node = format!("_:{}", b.as_str());
            if let Some(inverse) = iris(store, &node, v::SH_INVERSE_PATH).first() {
                return Some(Path::Inverse {
                    iri: inverse.clone(),
                });
            }
            let members = rdf_list(store, &node);
            if !members.is_empty() {
                let names: Vec<String> = members
                    .iter()
                    .map(|m| node_key(m).unwrap_or_else(|| "?".to_owned()))
                    .collect();
                return Some(Path::Complex {
                    description: names.join(" / "),
                });
            }
            Some(Path::Complex {
                description: "a path expression".to_owned(),
            })
        }
        Term::Literal(_) => None,
    }
}

/// Every datatype an `sh:or` of datatype constraints allows.
fn or_datatypes(store: &Store, subject: &str) -> Vec<String> {
    let mut out = Vec::new();
    for list in store.objects(subject, v::SH_OR) {
        let Some(head) = node_key(list) else { continue };
        for member in rdf_list(store, &head) {
            let Some(key) = node_key(&member) else {
                continue;
            };
            out.extend(iris(store, &key, v::SH_DATATYPE));
        }
    }
    out
}

/// The scheme membership hidden inside `sh:node [ sh:property [ … ] ]`.
fn nested_schemes(store: &Store, subject: &str, depth: usize) -> Vec<String> {
    if depth == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for node in store.objects(subject, v::SH_NODE) {
        let Some(key) = node_key(node) else { continue };
        for inner in store.objects(&key, v::SH_PROPERTY) {
            let Some(inner_key) = node_key(inner) else {
                continue;
            };
            let on_scheme = matches!(
                path(store, &inner_key),
                Some(Path::Predicate { ref iri }) if iri == v::SKOS_IN_SCHEME
            );
            if on_scheme {
                out.extend(iris(store, &inner_key, v::SH_HAS_VALUE));
            }
        }
        out.extend(nested_schemes(store, &key, depth - 1));
    }
    out.sort();
    out.dedup();
    out
}

fn property_shape(store: &Store, subject: &str) -> PropertyShape {
    let mut datatypes = iris(store, subject, v::SH_DATATYPE);
    datatypes.extend(or_datatypes(store, subject));
    datatypes.sort();
    datatypes.dedup();

    let values: Vec<String> = store
        .objects(subject, v::SH_IN)
        .into_iter()
        .filter_map(node_key)
        .flat_map(|head| rdf_list(store, &head))
        .map(|t| match t {
            Term::Literal(l) => l.value().to_owned(),
            Term::NamedNode(n) => n.as_str().to_owned(),
            Term::BlankNode(b) => format!("_:{}", b.as_str()),
        })
        .collect();

    let has_value: Vec<String> = store
        .objects(subject, v::SH_HAS_VALUE)
        .into_iter()
        .map(|t| match t {
            Term::Literal(l) => l.value().to_owned(),
            Term::NamedNode(n) => n.as_str().to_owned(),
            Term::BlankNode(b) => format!("_:{}", b.as_str()),
        })
        .collect();

    PropertyShape {
        path: path(store, subject),
        name: literal(store, subject, v::SH_NAME),
        description: literal(store, subject, v::SH_DESCRIPTION),
        datatypes,
        classes: iris(store, subject, v::SH_CLASS),
        node_kind: iris(store, subject, v::SH_NODE_KIND).first().cloned(),
        min_count: count(store, subject, v::SH_MIN_COUNT),
        max_count: count(store, subject, v::SH_MAX_COUNT),
        values,
        has_value,
        pattern: literal(store, subject, v::SH_PATTERN),
        in_scheme: nested_schemes(store, subject, 4),
        node: iris(store, subject, v::SH_NODE).first().cloned(),
        severity: iris(store, subject, v::SH_SEVERITY).first().cloned(),
        deactivated: boolean(store, subject, v::SH_DEACTIVATED).unwrap_or(false),
    }
}

impl Shapes {
    /// Read every named shape in the store.
    ///
    /// `domains` maps a property IRI to its `rdfs:domain` classes, which is
    /// what turns `sh:targetSubjectsOf` into a class the shape describes.
    pub fn extract(store: &Store, domains: &BTreeMap<String, Vec<String>>) -> Shapes {
        // A shape is anything declared one, or anything that carries a
        // property constraint or a target. An anonymous shape is reached
        // through its parent, never on its own.
        let mut subjects: BTreeSet<String> = BTreeSet::new();
        for t in &store.triples {
            let named = match &t.subject {
                oxrdf::NamedOrBlankNode::NamedNode(n) => n.as_str(),
                oxrdf::NamedOrBlankNode::BlankNode(_) => continue,
            };
            let declares_shape = t.predicate == v::RDF_TYPE
                && matches!(&t.object, Term::NamedNode(n) if n.as_str() == v::SH_NODE_SHAPE);
            let constrains = t.predicate == v::SH_PROPERTY
                || t.predicate == v::SH_TARGET_CLASS
                || t.predicate == v::SH_TARGET_SUBJECTS_OF
                || t.predicate == v::SH_TARGET_OBJECTS_OF;
            if declares_shape || constrains {
                subjects.insert(named.to_owned());
            }
        }

        let mut node_shapes: Vec<NodeShape> = subjects
            .into_iter()
            .map(|iri| {
                let target_classes = iris(store, &iri, v::SH_TARGET_CLASS);
                let target_subjects_of = iris(store, &iri, v::SH_TARGET_SUBJECTS_OF);
                let target_objects_of = iris(store, &iri, v::SH_TARGET_OBJECTS_OF);

                let mut applies_to = target_classes.clone();
                // An implicit class target: a shape that is also a class
                // constrains its own instances.
                if store
                    .objects(&iri, v::RDF_TYPE)
                    .iter()
                    .any(|t| matches!(t, Term::NamedNode(n) if n.as_str() == v::RDFS_CLASS || n.as_str() == v::OWL_CLASS))
                {
                    applies_to.push(iri.clone());
                }
                // The route this exists for.
                for property in &target_subjects_of {
                    if let Some(classes) = domains.get(property) {
                        applies_to.extend(classes.iter().cloned());
                    }
                }
                applies_to.sort();
                applies_to.dedup();

                let mut properties: Vec<PropertyShape> = store
                    .objects(&iri, v::SH_PROPERTY)
                    .into_iter()
                    .filter_map(node_key)
                    .map(|s| property_shape(store, &s))
                    .collect();
                // A shapes file is written in an order its author chose, but
                // blank nodes reach the model in parse order, which is not a
                // guarantee. Sort by what a reader would sort by.
                properties.sort_by(|a, b| {
                    (!a.required(), a.name.clone(), path_label(&a.path)).cmp(&(
                        !b.required(),
                        b.name.clone(),
                        path_label(&b.path),
                    ))
                });

                NodeShape {
                    target_nodes: iris(store, &iri, v::SH_TARGET_NODE),
                    closed: boolean(store, &iri, v::SH_CLOSED),
                    iri,
                    target_classes,
                    target_subjects_of,
                    target_objects_of,
                    applies_to,
                    properties,
                }
            })
            .collect();

        node_shapes.retain(|s| {
            !s.properties.is_empty()
                || !s.target_classes.is_empty()
                || !s.target_subjects_of.is_empty()
        });
        Shapes { node_shapes }
    }

    /// The shapes that describe instances of a class.
    pub fn for_class(&self, class_iri: &str) -> Vec<&NodeShape> {
        self.node_shapes
            .iter()
            .filter(|s| s.applies_to.iter().any(|c| c == class_iri))
            .collect()
    }

    /// Every constraint placed on one property, with the shape that places it.
    pub fn for_property(&self, property_iri: &str) -> Vec<(&NodeShape, &PropertyShape)> {
        self.node_shapes
            .iter()
            .flat_map(|s| s.properties.iter().map(move |p| (s, p)))
            .filter(|(_, p)| {
                p.path
                    .as_ref()
                    .and_then(Path::predicate)
                    .is_some_and(|i| i == property_iri)
            })
            .collect()
    }

    pub fn shape(&self, iri: &str) -> Option<&NodeShape> {
        self.node_shapes.iter().find(|s| s.iri == iri)
    }
}

/// A path as a sortable string.
fn path_label(path: &Option<Path>) -> String {
    match path {
        Some(Path::Predicate { iri }) => iri.clone(),
        Some(Path::Inverse { iri }) => format!("^{iri}"),
        Some(Path::Complex { description }) => description.clone(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinality_reads_the_way_a_person_writes_it() {
        let mut p = PropertyShape::default();
        // Saying nothing about counts is not the same as allowing any number,
        // so the page must be able to tell the difference.
        assert_eq!(p.cardinality(), None);
        assert!(!p.required());

        p.min_count = Some(1);
        p.max_count = Some(1);
        assert_eq!(p.cardinality().as_deref(), Some("1..1"));
        assert!(p.required());

        p.max_count = None;
        assert_eq!(p.cardinality().as_deref(), Some("1..*"));

        p.min_count = Some(0);
        p.max_count = Some(3);
        assert_eq!(p.cardinality().as_deref(), Some("0..3"));
        assert!(!p.required());
    }

    #[test]
    fn only_a_plain_predicate_path_joins_to_a_term() {
        assert_eq!(
            Path::Predicate {
                iri: "https://ex.org/a".to_owned()
            }
            .predicate(),
            Some("https://ex.org/a")
        );
        // An inverse path constrains the subjects of a property, not its
        // values, so putting it on the property's own page would misread it.
        assert_eq!(
            Path::Inverse {
                iri: "https://ex.org/a".to_owned()
            }
            .predicate(),
            None
        );
        assert_eq!(
            Path::Complex {
                description: "a / b".to_owned()
            }
            .predicate(),
            None
        );
    }
}
