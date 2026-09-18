//! JSON-LD output: a context per namespace, and every term and document
//! compacted against it.
//!
//! `docs/output-convention.md` asks for "the same triples compacted
//! with the namespace's `context.jsonld`". `oxjsonld` writes only the
//! streaming profile, whose keys are full predicate IRIs, so the compaction
//! here is our own. That is affordable because we also write the context: the
//! compactor never has to interpret a context it did not build.
//!
//! One deliberate departure from the convention. A term file inlines the
//! subset of the namespace context that its own triples use, instead of
//! referring to `context.jsonld` by URL. Compaction against a subset gives
//! exactly the same result as compaction against the whole, so the form is
//! unchanged, and the file becomes readable with `json.load` and no network.
//! A JSON-LD processor pointed at a URL must dereference it or fail, which
//! would make every term file useless offline.
//!
//! Correctness is measured, not asserted: `verify` parses what was written
//! and compares it against the triples that went in. Every JSON-LD file this
//! module produces goes through it during the build.

use crate::load::{Store, StoredTriple};
use crate::model::TermKind;
use crate::vocab;
use anyhow::{Result, bail};
use oxrdf::{NamedOrBlankNode, Term};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use super::Ctx;

/// How a key in the context maps to an IRI, and what it coerces values to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// The IRI the key expands to, written as a CURIE when one exists.
    id: String,
    /// `@type`: either `@id` or the CURIE of a datatype.
    coercion: Option<String>,
}

/// Which context entries a subset keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// Only terms used as a predicate, which is all a compacted file reads.
    PredicatesUsed,
    /// Every term mentioned, which is what a published context lists.
    EverythingTouched,
}

/// A JSON-LD context, and the reverse index compaction needs.
#[derive(Debug, Default, Clone)]
pub struct Context {
    prefixes: BTreeMap<String, String>,
    entries: BTreeMap<String, Entry>,
    /// Absolute IRI to context key.
    by_iri: BTreeMap<String, String>,
}

impl Context {
    /// The key a predicate or type IRI compacts to, if the context has one.
    fn key(&self, iri: &str) -> Option<&str> {
        self.by_iri.get(iri).map(String::as_str)
    }

    /// The CURIE of an IRI under this context's prefixes, if any.
    fn curie(&self, iri: &str) -> Option<String> {
        let (namespace, local) = vocab::split_iri(iri)?;
        if local.is_empty() {
            return None;
        }
        self.prefixes
            .iter()
            .find(|(_, ns)| ns.as_str() == namespace)
            .map(|(p, _)| format!("{p}:{local}"))
    }

    /// An IRI as short as this context allows: a CURIE if there is one, else
    /// the IRI unchanged.
    fn short(&self, iri: &str) -> String {
        self.curie(iri).unwrap_or_else(|| iri.to_owned())
    }

    fn coercion_of(&self, predicate: &str) -> Option<&str> {
        self.key(predicate)
            .and_then(|k| self.entries.get(k))
            .and_then(|e| e.coercion.as_deref())
    }

    /// A context document: the mapping under an `@context` key, which is the
    /// only shape a JSON-LD processor will accept when the file is
    /// dereferenced. A bare mapping is read as a node object instead, and
    /// its prefix declarations become triples.
    pub fn document(&self) -> Value {
        let mut root = Map::new();
        root.insert("@context".to_owned(), self.mapping());
        Value::Object(root)
    }

    /// The mapping itself, for inlining under a document's own `@context`.
    fn mapping(&self) -> Value {
        let mut map = Map::new();
        for (prefix, ns) in &self.prefixes {
            map.insert(prefix.clone(), Value::String(ns.clone()));
        }
        for (key, entry) in &self.entries {
            match &entry.coercion {
                None => {
                    map.insert(key.clone(), Value::String(entry.id.clone()));
                }
                Some(t) => {
                    let mut o = Map::new();
                    o.insert("@id".to_owned(), Value::String(entry.id.clone()));
                    o.insert("@type".to_owned(), Value::String(t.clone()));
                    map.insert(key.clone(), Value::Object(o));
                }
            }
        }
        Value::Object(map)
    }

    /// The subset of this context that the given triples actually use.
    ///
    /// `scope` decides which entries survive. A file inlines only the
    /// predicates it writes as keys, because an entry for a term that appears
    /// merely as a value is never consulted and would drag most of the
    /// vocabulary into every SHACL shape. A namespace publishes every term it
    /// touches, which is what a context is for.
    fn subset(&self, triples: &[&StoredTriple], scope: Scope) -> Context {
        let mut iris: BTreeSet<&str> = BTreeSet::new();
        let mut predicates: BTreeSet<&str> = BTreeSet::new();
        let mut subjects: BTreeSet<&str> = BTreeSet::new();
        let mut namespaces: BTreeSet<&str> = BTreeSet::new();
        for t in triples {
            iris.insert(t.predicate.as_str());
            predicates.insert(t.predicate.as_str());
            if let NamedOrBlankNode::NamedNode(n) = &t.subject {
                // A subject is listed but never abbreviated: `@id` is always
                // the absolute IRI, because it is the identity a reader keys
                // on. So it earns a context entry but not a prefix.
                iris.insert(n.as_str());
                subjects.insert(n.as_str());
            }
            match &t.object {
                Term::NamedNode(n) => {
                    iris.insert(n.as_str());
                }
                Term::Literal(l) => {
                    iris.insert(l.datatype().as_str());
                }
                Term::BlankNode(_) => {}
            }
        }
        // `rdf:type` is written as the `@type` keyword and the implicit
        // datatypes are never written, so none of them earns a prefix.
        for iri in &iris {
            if *iri == vocab::RDF_TYPE
                || *iri == vocab::RDF_LANG_STRING
                || *iri == concat!("http://www.w3.org/2001/XMLSchema#", "string")
                || (subjects.contains(iri) && !predicates.contains(iri))
            {
                continue;
            }
            if let Some((ns, _)) = vocab::split_iri(iri) {
                namespaces.insert(ns);
            }
        }

        let wanted = match scope {
            Scope::PredicatesUsed => &predicates,
            Scope::EverythingTouched => &iris,
        };
        let by_iri: BTreeMap<String, String> = self
            .by_iri
            .iter()
            .filter(|(iri, _)| wanted.contains(iri.as_str()))
            .map(|(i, k)| (i.clone(), k.clone()))
            .collect();
        let entries: BTreeMap<String, Entry> = by_iri
            .values()
            .filter_map(|k| self.entries.get(k).map(|e| (k.clone(), e.clone())))
            .collect();

        // A prefix earns its place by being needed: for a namespace some IRI
        // lives in, or for a CURIE an entry is written as.
        let mut prefixes: BTreeMap<String, String> = self
            .prefixes
            .iter()
            .filter(|(_, ns)| namespaces.contains(ns.as_str()))
            .map(|(p, ns)| (p.clone(), ns.clone()))
            .collect();
        for entry in entries.values() {
            for value in [Some(entry.id.as_str()), entry.coercion.as_deref()] {
                let Some(v) = value else { continue };
                let Some((prefix, _)) = v.split_once(':') else {
                    continue;
                };
                if let Some(ns) = self.prefixes.get(prefix) {
                    prefixes.insert(prefix.to_owned(), ns.clone());
                }
            }
        }

        Context {
            prefixes,
            entries,
            by_iri,
        }
    }
}

/// Whether a range IRI names a literal datatype rather than a class.
fn is_datatype(iri: &str) -> bool {
    iri.starts_with(vocab::XSD) || iri == vocab::RDF_LANG_STRING
}

/// Build the context for one release.
///
/// Keys are local names, which is what makes the compacted output read like
/// JSON rather than like RDF. Two terms in different namespaces can share a
/// local name; the local vocabulary keeps the plain key and the other takes
/// its CURIE, so a key never silently means two things.
pub fn context(ctx: &Ctx<'_>) -> Context {
    let mut prefixes = ctx.release.prefixes.clone();
    // Compaction writes datatypes as CURIEs, so xsd must be declarable even
    // when no input file declared it.
    prefixes
        .entry("xsd".to_owned())
        .or_insert_with(|| vocab::XSD.to_owned());

    let mut out = Context {
        prefixes,
        ..Context::default()
    };

    // Local terms first, so that on a collision they keep the plain key.
    let mut ordered: Vec<&crate::model::Term> = ctx.release.terms.iter().collect();
    ordered.sort_by(|a, b| (a.foreign, &a.iri).cmp(&(b.foreign, &b.iri)));

    for term in ordered {
        let curie = out.short(&term.iri);
        let plain = vocab::local_name(&term.iri).to_owned();
        // A term takes its local name when that name is still free, and its
        // CURIE otherwise, so a key never silently means two things.
        let key = if out.entries.contains_key(&plain) {
            curie.clone()
        } else {
            plain
        };
        if out.entries.contains_key(&key) {
            continue;
        }

        let coercion = coercion_for(term.kind, &term.property).map(|c| match c.as_str() {
            "@id" => "@id".to_owned(),
            other => out.short(other),
        });

        out.by_iri.insert(term.iri.clone(), key.clone());
        out.entries.insert(
            key,
            Entry {
                id: curie,
                coercion,
            },
        );
    }
    out
}

/// What a property's values should be read as, from its declared range.
///
/// Only a declared range earns a coercion. Guessing from the values that
/// happen to be present would make the context depend on the data, so adding
/// one triple could change how every other value is read.
fn coercion_for(kind: TermKind, property: &crate::model::PropertyFacts) -> Option<String> {
    let ranges: Vec<&String> = property
        .range
        .iter()
        .chain(property.range_includes.iter())
        .collect();
    match kind {
        TermKind::ObjectProperty => Some("@id".to_owned()),
        TermKind::DatatypeProperty | TermKind::RdfProperty | TermKind::AnnotationProperty => {
            match ranges.as_slice() {
                [only] if is_datatype(only) => Some((*only).clone()),
                [_] => Some("@id".to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A context document, serialised and checked.
///
/// A context carries no assertions, so it must expand to nothing. Written as
/// a bare mapping instead of under an `@context` key it expands to a node
/// object, and every prefix declaration becomes a triple: six of the ten
/// contexts in the first build did exactly that, and only an independent
/// processor noticed.
pub fn context_document(context: &Context) -> Result<String> {
    let value = context.document();
    verify(&value, &[])?;
    Ok(serde_json::to_string_pretty(&value)?)
}

/// The context a single namespace publishes: the entries its own document and
/// terms actually use, so the file describes that vocabulary and not the whole
/// release.
pub fn namespace_context(
    ctx: &Ctx<'_>,
    context: &Context,
    ns: &crate::site::NamespacePlan,
) -> Result<String> {
    let mut triples: Vec<&StoredTriple> = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(doc) = ns.document.as_deref().and_then(|i| ctx.release.document(i)) {
        triples.extend(
            ctx.store
                .triples
                .iter()
                .filter(|t| t.file == doc.source_file),
        );
    }
    for term in ctx.release.local_terms().filter(|t| t.namespace == ns.iri) {
        triples.extend(bounded(ctx.store, &term.iri, &mut seen));
    }
    context_document(&context.subset(&triples, Scope::EverythingTouched))
}

/// Collect the concise bounded description of a subject.
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

fn subject_key(s: &NamedOrBlankNode) -> String {
    match s {
        NamedOrBlankNode::NamedNode(n) => n.as_str().to_owned(),
        NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
    }
}

/// One literal or IRI, written the shortest way the context makes safe.
fn value(context: &Context, predicate: &str, object: &Term) -> Value {
    let coercion = context.coercion_of(predicate);
    match object {
        Term::NamedNode(n) => {
            let short = context.short(n.as_str());
            if coercion == Some("@id") {
                Value::String(short)
            } else {
                let mut o = Map::new();
                o.insert("@id".to_owned(), Value::String(short));
                Value::Object(o)
            }
        }
        Term::BlankNode(b) => {
            let mut o = Map::new();
            o.insert("@id".to_owned(), Value::String(format!("_:{}", b.as_str())));
            Value::Object(o)
        }
        Term::Literal(l) => {
            let datatype = l.datatype().as_str();
            if let Some(lang) = l.language() {
                let mut o = Map::new();
                o.insert("@value".to_owned(), Value::String(l.value().to_owned()));
                o.insert("@language".to_owned(), Value::String(lang.to_owned()));
                return Value::Object(o);
            }
            let plain_string = datatype == concat!("http://www.w3.org/2001/XMLSchema#", "string");
            // A plain string is only safe bare when nothing would re-read it
            // as an IRI or as another datatype.
            if plain_string && coercion.is_none() {
                return Value::String(l.value().to_owned());
            }
            // JSON's own booleans and integers expand back to exactly these
            // datatypes, so a reader with no JSON-LD processor still gets a
            // usable value. Only canonical lexical forms qualify, since
            // `"01"^^xsd:integer` would not survive the round trip; anything
            // else keeps its wrapper and `verify` would catch a mistake here.
            if coercion != Some("@id") {
                if datatype == concat!("http://www.w3.org/2001/XMLSchema#", "boolean") {
                    match l.value() {
                        "true" => return Value::Bool(true),
                        "false" => return Value::Bool(false),
                        _ => {}
                    }
                }
                if datatype == concat!("http://www.w3.org/2001/XMLSchema#", "integer")
                    && let Ok(n) = l.value().parse::<i64>()
                    && n.to_string() == l.value()
                {
                    return Value::Number(n.into());
                }
            }
            if coercion == Some(context.short(datatype).as_str()) {
                return Value::String(l.value().to_owned());
            }
            let mut o = Map::new();
            o.insert("@value".to_owned(), Value::String(l.value().to_owned()));
            if !plain_string {
                o.insert("@type".to_owned(), Value::String(context.short(datatype)));
            }
            Value::Object(o)
        }
    }
}

/// Group triples by subject, keeping a stable order.
fn by_subject<'a>(triples: &[&'a StoredTriple]) -> BTreeMap<String, Vec<&'a StoredTriple>> {
    let mut out: BTreeMap<String, Vec<&StoredTriple>> = BTreeMap::new();
    for t in triples {
        out.entry(subject_key(&t.subject)).or_default().push(t);
    }
    out
}

/// One node object. Blank nodes reached from it are embedded; `stack` stops a
/// cycle, and `embedded` stops a node being written twice.
fn node(
    context: &Context,
    groups: &BTreeMap<String, Vec<&StoredTriple>>,
    subject: &str,
    stack: &mut BTreeSet<String>,
    embedded: &mut BTreeSet<String>,
    embed_blanks: bool,
) -> Option<Value> {
    if !stack.insert(subject.to_owned()) {
        return None; // a cycle: this graph cannot be a tree
    }
    let mut map = Map::new();
    // A nested blank node needs no label; anything else is identified.
    if !subject.starts_with("_:") || !embed_blanks {
        map.insert("@id".to_owned(), Value::String(subject.to_owned()));
    }

    let empty = Vec::new();
    let triples = groups.get(subject).unwrap_or(&empty);

    // `rdf:type` is the `@type` keyword, never a predicate key.
    let mut types: Vec<String> = triples
        .iter()
        .filter(|t| t.predicate == vocab::RDF_TYPE)
        .filter_map(|t| match &t.object {
            Term::NamedNode(n) => Some(context.short(n.as_str())),
            _ => None,
        })
        .collect();
    types.sort();
    types.dedup();
    match types.len() {
        0 => {}
        1 => {
            map.insert("@type".to_owned(), Value::String(types.remove(0)));
        }
        _ => {
            map.insert(
                "@type".to_owned(),
                Value::Array(types.into_iter().map(Value::String).collect()),
            );
        }
    }

    // Predicates, keyed as the context allows, sorted by that key.
    let mut buckets: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for t in triples {
        if t.predicate == vocab::RDF_TYPE {
            continue;
        }
        let key = context
            .key(&t.predicate)
            .map(str::to_owned)
            .unwrap_or_else(|| context.short(&t.predicate));

        let v = match &t.object {
            Term::BlankNode(b) if embed_blanks => {
                let id = format!("_:{}", b.as_str());
                if embedded.contains(&id) {
                    stack.remove(subject);
                    return None; // shared: embedding would duplicate it
                }
                embedded.insert(id.clone());
                match node(context, groups, &id, stack, embedded, embed_blanks) {
                    Some(v) => v,
                    None => {
                        stack.remove(subject);
                        return None;
                    }
                }
            }
            other => value(context, &t.predicate, other),
        };
        buckets.entry(key).or_default().push(v);
    }

    for (key, mut values) in buckets {
        // RDF has no order, so the file needs one of its own.
        values.sort_by_key(|v| v.to_string());
        let v = if values.len() == 1 {
            values.remove(0)
        } else {
            Value::Array(values)
        };
        map.insert(key, v);
    }

    stack.remove(subject);
    Some(Value::Object(map))
}

/// Build a JSON-LD document for one root subject, or for a whole graph.
fn document_value(context: &Context, triples: &[&StoredTriple], roots: &[String]) -> Value {
    let groups = by_subject(triples);
    let subset = context.subset(triples, Scope::PredicatesUsed);

    // First try the readable form, with blank nodes nested where they are
    // used. A shared blank node or a cycle rules it out, and then every node
    // is written flat with its own label.
    let embedded = |embed: bool| -> Option<Vec<Value>> {
        let mut written = BTreeSet::new();
        let mut nodes = Vec::new();
        for r in roots {
            let mut stack = BTreeSet::new();
            nodes.push(node(&subset, &groups, r, &mut stack, &mut written, embed)?);
        }
        if !embed {
            for s in groups.keys() {
                if roots.contains(s) {
                    continue;
                }
                let mut stack = BTreeSet::new();
                nodes.push(node(&subset, &groups, s, &mut stack, &mut written, embed)?);
            }
        }
        Some(nodes)
    };

    let (mut nodes, flat) = match embedded(true) {
        Some(n) => (n, false),
        None => (embedded(false).unwrap_or_default(), true),
    };

    let mut root = Map::new();
    root.insert("@context".to_owned(), subset.mapping());
    if nodes.len() == 1 && !flat {
        let Value::Object(only) = nodes.remove(0) else {
            unreachable!("a node is always an object")
        };
        for (k, v) in only {
            root.insert(k, v);
        }
    } else {
        root.insert("@graph".to_owned(), Value::Array(nodes));
    }
    Value::Object(root)
}

/// The JSON-LD of one term: its concise bounded description, compacted.
pub fn term(ctx: &Ctx<'_>, context: &Context, iri: &str) -> Result<String> {
    let mut seen = BTreeSet::new();
    let triples = bounded(ctx.store, iri, &mut seen);
    let value = document_value(context, &triples, &[iri.to_owned()]);
    verify(&value, &triples)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

/// The JSON-LD of every triple in one source file.
pub fn document(ctx: &Ctx<'_>, context: &Context, file: usize) -> Result<String> {
    let triples: Vec<&StoredTriple> = ctx
        .store
        .triples
        .iter()
        .filter(|t| t.file == file)
        .collect();
    graph(context, &triples)
}

/// The JSON-LD of the whole release.
pub fn release(ctx: &Ctx<'_>, context: &Context) -> Result<String> {
    let triples: Vec<&StoredTriple> = ctx.store.triples.iter().collect();
    graph(context, &triples)
}

fn graph(context: &Context, triples: &[&StoredTriple]) -> Result<String> {
    // A named subject is a root; a blank node is reached through one.
    let mut roots: Vec<String> = triples
        .iter()
        .filter_map(|t| match &t.subject {
            NamedOrBlankNode::NamedNode(n) => Some(n.as_str().to_owned()),
            NamedOrBlankNode::BlankNode(_) => None,
        })
        .collect();
    roots.sort();
    roots.dedup();
    let value = document_value(context, triples, &roots);
    verify(&value, triples)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

/// A triple with every blank node replaced, for comparison.
///
/// A JSON-LD parser mints its own blank node labels, so the labels cannot be
/// compared. Everything else can, which is enough to catch a lost triple, a
/// wrong datatype, a predicate compacted to the wrong key, or a plain string
/// silently re-read as an IRI.
fn signature(subject: &str, predicate: &str, object: &Term) -> String {
    let s = if subject.starts_with("_:") {
        "_:"
    } else {
        subject
    };
    let o = match object {
        Term::BlankNode(_) => "_:".to_owned(),
        Term::NamedNode(n) => format!("<{}>", n.as_str()),
        Term::Literal(l) => match l.language() {
            Some(lang) => format!("{:?}@{lang}", l.value()),
            None => format!("{:?}^^{}", l.value(), l.datatype().as_str()),
        },
    };
    format!("{s} <{predicate}> {o}")
}

/// Parse what was written and compare it with what went in.
///
/// This runs on every JSON-LD file the build produces. Compaction is our own
/// code reading a context our own code wrote, which is exactly the situation
/// where a mistake is invisible: the output stays valid JSON and plausible
/// JSON-LD while meaning something else.
pub fn verify(value: &Value, expected: &[&StoredTriple]) -> Result<()> {
    use oxrdfio::{JsonLdProfileSet, RdfFormat, RdfParser};

    let text = serde_json::to_vec(value)?;
    let parser = RdfParser::from_format(RdfFormat::JsonLd {
        profile: JsonLdProfileSet::empty(),
    });

    let mut got: Vec<String> = Vec::new();
    for quad in parser.for_reader(text.as_slice()) {
        let quad = quad?;
        got.push(signature(
            &subject_key(&quad.subject),
            quad.predicate.as_str(),
            &quad.object,
        ));
    }
    let mut want: Vec<String> = expected
        .iter()
        .map(|t| signature(&subject_key(&t.subject), &t.predicate, &t.object))
        .collect();

    // Compare as sets, because RDF is a set. The same triple stated in two
    // input files is one triple, and the writer emits it once; comparing
    // multisets made `iyo build a/ b/` fail with "wrote 197 triples, read
    // back 182" and nothing named as lost or invented, because nothing was:
    // both directories described the same release. Blank node labels are
    // canonicalised in `load`, so two structurally identical triples with
    // distinct blank nodes keep distinct signatures and survive this.
    got.sort();
    got.dedup();
    want.sort();
    want.dedup();
    if got == want {
        return Ok(());
    }

    let missing: Vec<&String> = want.iter().filter(|s| !got.contains(s)).take(3).collect();
    let extra: Vec<&String> = got.iter().filter(|s| !want.contains(s)).take(3).collect();
    bail!(
        "JSON-LD does not round-trip: wrote {} triples, read back {}{}{}",
        want.len(),
        got.len(),
        if missing.is_empty() {
            String::new()
        } else {
            format!("; lost {missing:?}")
        },
        if extra.is_empty() {
            String::new()
        } else {
            format!("; invented {extra:?}")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_string_is_bare_only_when_nothing_would_re_read_it() {
        let mut c = Context::default();
        c.prefixes
            .insert("ex".to_owned(), "https://ex.org/".to_owned());
        c.entries.insert(
            "note".to_owned(),
            Entry {
                id: "ex:note".to_owned(),
                coercion: None,
            },
        );
        c.by_iri
            .insert("https://ex.org/note".to_owned(), "note".to_owned());
        c.entries.insert(
            "seeAlso".to_owned(),
            Entry {
                id: "ex:seeAlso".to_owned(),
                coercion: Some("@id".to_owned()),
            },
        );
        c.by_iri
            .insert("https://ex.org/seeAlso".to_owned(), "seeAlso".to_owned());

        let lit = Term::Literal(oxrdf::Literal::new_simple_literal("hello"));
        assert_eq!(
            value(&c, "https://ex.org/note", &lit),
            Value::String("hello".to_owned())
        );
        // The same literal under a property coerced to @id must keep its
        // wrapper, or reading the file back turns it into an IRI.
        let wrapped = value(&c, "https://ex.org/seeAlso", &lit);
        assert_eq!(wrapped["@value"], "hello");
    }

    #[test]
    fn an_iri_under_an_id_coercion_loses_its_wrapper() {
        let mut c = Context::default();
        c.prefixes
            .insert("ex".to_owned(), "https://ex.org/".to_owned());
        c.entries.insert(
            "broader".to_owned(),
            Entry {
                id: "ex:broader".to_owned(),
                coercion: Some("@id".to_owned()),
            },
        );
        c.by_iri
            .insert("https://ex.org/broader".to_owned(), "broader".to_owned());
        let obj = Term::NamedNode(oxrdf::NamedNode::new("https://ex.org/thing").unwrap());
        assert_eq!(
            value(&c, "https://ex.org/broader", &obj),
            Value::String("ex:thing".to_owned())
        );
        // Without the coercion the same object keeps its wrapper.
        assert_eq!(value(&c, "https://ex.org/other", &obj)["@id"], "ex:thing");
    }

    #[test]
    fn only_a_declared_range_earns_a_coercion() {
        use crate::model::PropertyFacts;
        let mut p = PropertyFacts::default();
        assert_eq!(coercion_for(TermKind::AnnotationProperty, &p), None);
        // An object property is coerced whether or not it declares a range.
        assert_eq!(
            coercion_for(TermKind::ObjectProperty, &p),
            Some("@id".to_owned())
        );
        p.range = vec![format!("{}date", vocab::XSD)];
        assert_eq!(
            coercion_for(TermKind::AnnotationProperty, &p),
            Some("http://www.w3.org/2001/XMLSchema#date".to_owned())
        );
        p.range = vec!["https://ex.org/Class".to_owned()];
        assert_eq!(
            coercion_for(TermKind::RdfProperty, &p),
            Some("@id".to_owned())
        );
        // Two ranges say nothing definite, so neither does the context.
        p.range.push(format!("{}date", vocab::XSD));
        assert_eq!(coercion_for(TermKind::DatatypeProperty, &p), None);
    }
}
