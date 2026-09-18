//! Turtle output: a concise bounded description per term, and the graph of a
//! whole document.
//!
//! Only the prefixes a snippet actually uses are declared, so a per-term file
//! is readable on its own.

use crate::load::{Store, StoredTriple};
use anyhow::Result;
use oxrdf::{NamedNodeRef, NamedOrBlankNode, Term, TripleRef};
use oxrdfio::{RdfFormat, RdfSerializer};
use std::collections::{BTreeMap, BTreeSet};

/// Collect the concise bounded description of a subject: its own triples, and
/// recursively the triples of any blank node it reaches.
fn bounded<'a>(
    store: &'a Store,
    subject: &str,
    seen: &mut BTreeSet<String>,
) -> Vec<&'a StoredTriple> {
    if !seen.insert(subject.to_owned()) {
        return Vec::new();
    }
    let mut out: Vec<&StoredTriple> = store.about(subject).collect();
    let blanks: Vec<String> = out
        .iter()
        .filter_map(|t| match &t.object {
            Term::BlankNode(b) => Some(format!("_:{}", b.as_str())),
            _ => None,
        })
        .collect();
    for b in blanks {
        out.extend(bounded(store, &b, seen));
    }
    out
}

fn namespaces_used(triples: &[&StoredTriple]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |iri: &str| {
        if let Some((ns, _)) = crate::vocab::split_iri(iri) {
            out.insert(ns.to_owned());
        }
    };
    for t in triples {
        if let NamedOrBlankNode::NamedNode(n) = &t.subject {
            add(n.as_str());
        }
        // `rdf:type` is written `a`, and the implicit string datatypes are
        // never written, so neither forces a prefix declaration.
        if t.predicate != crate::vocab::RDF_TYPE {
            add(&t.predicate);
        }
        match &t.object {
            Term::NamedNode(n) => add(n.as_str()),
            Term::Literal(l) => {
                let dt = l.datatype().as_str();
                if dt != crate::vocab::RDF_LANG_STRING
                    && dt != concat!("http://www.w3.org/2001/XMLSchema#", "string")
                {
                    add(dt);
                }
            }
            _ => {}
        }
    }
    out
}

fn serialize(triples: &[&StoredTriple], prefixes: &BTreeMap<String, String>) -> Result<String> {
    let used = namespaces_used(triples);
    let mut serializer = RdfSerializer::from_format(RdfFormat::Turtle);
    for (prefix, namespace) in prefixes {
        if used.contains(namespace) {
            serializer = serializer.with_prefix(prefix, namespace)?;
        }
    }
    let mut writer = serializer.for_writer(Vec::new());
    for t in triples {
        writer.serialize_triple(TripleRef::new(
            t.subject.as_ref(),
            NamedNodeRef::new_unchecked(t.predicate.as_str()),
            t.object.as_ref(),
        ))?;
    }
    Ok(String::from_utf8(writer.finish()?)?)
}

/// The Turtle of one term, as its own document.
pub fn term(store: &Store, subject: &str, prefixes: &BTreeMap<String, String>) -> Result<String> {
    let mut seen = BTreeSet::new();
    let triples = bounded(store, subject, &mut seen);
    serialize(&triples, prefixes)
}

/// The Turtle of everything that came from one input file.
pub fn document(
    store: &Store,
    file_index: usize,
    prefixes: &BTreeMap<String, String>,
) -> Result<String> {
    let triples: Vec<&StoredTriple> = store
        .triples
        .iter()
        .filter(|t| t.file == file_index)
        .collect();
    serialize(&triples, prefixes)
}

/// The Turtle of the whole release: every input file unioned, which is the
/// graph a validator needs.
pub fn release(store: &Store, prefixes: &BTreeMap<String, String>) -> Result<String> {
    let triples: Vec<&StoredTriple> = store.triples.iter().collect();
    serialize(&triples, prefixes)
}
