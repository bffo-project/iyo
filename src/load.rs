//! Stage 1 of the pipeline: read every input file into one indexed store,
//! keeping the source file of every triple and every file's prefix map.
//!
//! Provenance is per file, not per line: `oxrdfio` reports positions on parse
//! *errors* but does not carry them on successfully parsed quads, so findings
//! cite a file and a subject.

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use oxrdf::{BlankNode, NamedOrBlankNode, Term};
use oxrdfio::{RdfFormat, RdfParser};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

/// The input that means "read RDF from stdin" (clig.dev G15). A path,
/// because it travels through the whole pipeline as one, and because it is
/// what a finding should cite as the source of a triple that arrived that
/// way.
pub const STDIN: &str = "-";

/// One parsed input file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: Utf8PathBuf,
    pub format: &'static str,
    /// Prefix declarations found in this file, prefix to namespace IRI.
    pub prefixes: BTreeMap<String, String>,
    pub base_iri: Option<String>,
    pub triple_count: usize,
}

/// A triple plus the index of the file it came from.
#[derive(Debug, Clone)]
pub struct StoredTriple {
    pub file: usize,
    pub subject: NamedOrBlankNode,
    pub predicate: String,
    pub object: Term,
}

/// Every input file loaded into one store, indexed for the lookups the model
/// builder and the checks need.
#[derive(Debug, Default)]
pub struct Store {
    pub files: Vec<SourceFile>,
    pub triples: Vec<StoredTriple>,
    by_subject: HashMap<String, Vec<usize>>,
    by_predicate: HashMap<String, Vec<usize>>,
    by_predicate_object: HashMap<(String, String), Vec<usize>>,
}

/// The key under which a subject or a node-valued object is indexed.
pub fn node_key(node: &NamedOrBlankNode) -> String {
    match node {
        NamedOrBlankNode::NamedNode(n) => n.as_str().to_owned(),
        NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
    }
}

/// The index key of a term, or `None` for literals.
pub fn term_key(term: &Term) -> Option<String> {
    match term {
        Term::NamedNode(n) => Some(n.as_str().to_owned()),
        Term::BlankNode(b) => Some(format!("_:{}", b.as_str())),
        _ => None,
    }
}

/// The IRI of a term, or `None` for literals and blank nodes.
pub fn term_iri(term: &Term) -> Option<&str> {
    match term {
        Term::NamedNode(n) => Some(n.as_str()),
        _ => None,
    }
}

impl Store {
    fn index(&mut self) {
        self.by_subject.clear();
        self.by_predicate.clear();
        self.by_predicate_object.clear();
        for (i, t) in self.triples.iter().enumerate() {
            self.by_subject
                .entry(node_key(&t.subject))
                .or_default()
                .push(i);
            self.by_predicate
                .entry(t.predicate.clone())
                .or_default()
                .push(i);
            if let Some(o) = term_key(&t.object) {
                self.by_predicate_object
                    .entry((t.predicate.clone(), o))
                    .or_default()
                    .push(i);
            }
        }
    }

    /// Every triple with this subject, in input order.
    pub fn about(&self, subject: &str) -> impl Iterator<Item = &StoredTriple> {
        self.by_subject
            .get(subject)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|i| &self.triples[*i])
    }

    /// The objects of `subject predicate ?o`, in input order.
    pub fn objects(&self, subject: &str, predicate: &str) -> Vec<&Term> {
        self.about(subject)
            .filter(|t| t.predicate == predicate)
            .map(|t| &t.object)
            .collect()
    }

    /// The first object of `subject predicate ?o`.
    pub fn object(&self, subject: &str, predicate: &str) -> Option<&Term> {
        self.about(subject)
            .find(|t| t.predicate == predicate)
            .map(|t| &t.object)
    }

    /// The IRI objects of `subject predicate ?o`, sorted and deduplicated.
    pub fn iri_objects(&self, subject: &str, predicate: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .objects(subject, predicate)
            .into_iter()
            .filter_map(|t| term_iri(t).map(str::to_owned))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Subjects that carry this predicate at all, sorted.
    pub fn subjects_of(&self, predicate: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .by_predicate
            .get(predicate)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|i| node_key(&self.triples[*i].subject))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Subjects of `?s predicate object`, sorted.
    pub fn subjects_with(&self, predicate: &str, object: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .by_predicate_object
            .get(&(predicate.to_owned(), object.to_owned()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|i| node_key(&self.triples[*i].subject))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The `rdf:type` IRIs of a subject, sorted.
    pub fn types(&self, subject: &str) -> Vec<String> {
        self.iri_objects(subject, crate::vocab::RDF_TYPE)
    }

    pub fn has_type(&self, subject: &str, class: &str) -> bool {
        self.objects(subject, crate::vocab::RDF_TYPE)
            .into_iter()
            .any(|t| term_iri(t) == Some(class))
    }

    /// The first file in which this subject appears, which is the file that
    /// defines it for term-to-document assignment.
    pub fn file_of(&self, subject: &str) -> Option<usize> {
        self.about(subject).map(|t| t.file).min()
    }

    /// Every named subject in the store, sorted.
    pub fn named_subjects(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .by_subject
            .keys()
            .filter(|k| !k.starts_with("_:"))
            .cloned()
            .collect();
        v.sort();
        v
    }

    /// Every namespace IRI actually used by any subject, predicate or IRI
    /// object, for the declared-but-unused prefix check.
    pub fn used_namespaces(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut add = |iri: &str| {
            if let Some((ns, _)) = crate::vocab::split_iri(iri) {
                out.insert(ns.to_owned());
            }
        };
        for t in &self.triples {
            if let NamedOrBlankNode::NamedNode(n) = &t.subject {
                add(n.as_str());
            }
            add(&t.predicate);
            match &t.object {
                Term::NamedNode(n) => add(n.as_str()),
                Term::Literal(l) => add(l.datatype().as_str()),
                _ => {}
            }
        }
        out
    }

    /// The union of every file's prefix map. Later files do not overwrite
    /// earlier ones; a conflict is reported by `check`.
    pub fn prefixes(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for f in &self.files {
            for (p, ns) in &f.prefixes {
                out.entry(p.clone()).or_insert_with(|| ns.clone());
            }
        }
        out
    }
}

fn format_for(path: &Utf8Path) -> Option<(RdfFormat, &'static str)> {
    let ext = path.extension()?.to_ascii_lowercase();
    // `owl` is conventionally RDF/XML but is not in oxrdfio's extension table.
    let fmt = match ext.as_str() {
        "owl" | "xml" => RdfFormat::RdfXml,
        other => RdfFormat::from_extension(other)?,
    };
    let name = match fmt {
        RdfFormat::Turtle => "turtle",
        RdfFormat::NTriples => "ntriples",
        RdfFormat::NQuads => "nquads",
        RdfFormat::TriG => "trig",
        RdfFormat::RdfXml => "rdfxml",
        RdfFormat::N3 => "n3",
        RdfFormat::JsonLd { .. } => "jsonld",
        _ => "rdf",
    };
    Some((fmt, name))
}

/// True when the path looks like an RDF file `iyo` can parse.
pub fn is_rdf_file(path: &Utf8Path) -> bool {
    format_for(path).is_some()
}

/// Match a file name against a pattern whose only wildcard is `*`.
///
/// A dependency-free replacement for a glob crate: `globset` 0.4.20 raises the
/// minimum Rust version to 1.88 while the rest of the stack builds on 1.87, and
/// the design asks for a small dependency set.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    fn go(n: &[u8], p: &[u8]) -> bool {
        match (n.first(), p.first()) {
            (_, Some(b'*')) => go(n, &p[1..]) || (!n.is_empty() && go(&n[1..], p)),
            (Some(a), Some(b)) if a == b => go(&n[1..], &p[1..]),
            (None, None) => true,
            _ => false,
        }
    }
    go(name.as_bytes(), pattern.as_bytes())
}

/// Turn command-line or configuration inputs into a sorted list of files.
///
/// Accepts files, directories (every RDF file directly inside, not recursive)
/// and patterns containing `*` in the last path segment.
pub fn expand_inputs(inputs: &[String], base: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let mut out: Vec<Utf8PathBuf> = Vec::new();
    for raw in inputs {
        // `-` is stdin, not a file called `-` in the working directory, and
        // it can only appear once: stdin is read to the end the first time.
        if raw == STDIN {
            if out.iter().any(|p| p == STDIN) {
                return Err(crate::Failure::err(
                    crate::exit::USAGE,
                    "- given more than once; stdin can only be read to the end once",
                    "pass - at most once, with file paths for the other inputs",
                ));
            }
            out.push(Utf8PathBuf::from(STDIN));
            continue;
        }
        let joined = if Utf8Path::new(raw).is_absolute() {
            Utf8PathBuf::from(raw)
        } else {
            base.join(raw)
        };
        if joined.as_str().contains('*') {
            let dir = joined.parent().unwrap_or(base).to_owned();
            let pattern = joined.file_name().unwrap_or_default().to_owned();
            let entries =
                std::fs::read_dir(&dir).with_context(|| format!("reading directory {dir}"))?;
            let mut matched = Vec::new();
            for e in entries {
                let e = e?;
                let p = Utf8PathBuf::from_path_buf(e.path())
                    .map_err(|p| anyhow::anyhow!("path is not UTF-8: {}", p.display()))?;
                let name = p.file_name().unwrap_or_default();
                if matches_pattern(name, &pattern) && p.is_file() {
                    matched.push(p);
                }
            }
            if matched.is_empty() {
                return Err(crate::Failure::err(
                    crate::exit::INPUT,
                    format!("no files matched {joined}"),
                    "quote the pattern so the shell does not expand it first, \
                     as in 'vocabularies/*.ttl'",
                ));
            }
            matched.sort();
            out.extend(matched);
        } else if joined.is_dir() {
            let mut matched = Vec::new();
            for e in
                std::fs::read_dir(&joined).with_context(|| format!("reading directory {joined}"))?
            {
                let e = e?;
                let p = Utf8PathBuf::from_path_buf(e.path())
                    .map_err(|p| anyhow::anyhow!("path is not UTF-8: {}", p.display()))?;
                if p.is_file() && is_rdf_file(&p) {
                    matched.push(p);
                }
            }
            if matched.is_empty() {
                return Err(crate::Failure::err(
                    crate::exit::INPUT,
                    format!("no RDF files in {joined}"),
                    "iyo reads .ttl, .owl, .rdf, .nt and .jsonld; name a file directly \
                     if the extension is something else",
                ));
            }
            matched.sort();
            out.extend(matched);
        } else if joined.is_file() {
            out.push(joined);
        } else {
            return Err(crate::Failure::err(
                crate::exit::INPUT,
                format!("no such file or directory: {joined}"),
                "check the path, or pass - to read RDF from stdin",
            ));
        }
    }
    out.dedup();
    // Report paths as given, relative to the base, so findings are readable
    // and golden files do not embed an absolute path.
    Ok(out
        .into_iter()
        .map(|p| p.strip_prefix(base).map(Utf8Path::to_owned).unwrap_or(p))
        .collect())
}

/// Give every blank node a label derived from the order it first appears in.
///
/// Turtle parsers mint fresh identifiers for anonymous nodes (`[ ... ]`), and
/// those identifiers are not stable between runs, so they would leak into the
/// model and break byte-identical rebuilds. Parse order is stable, so numbering
/// by first appearance is.
fn canonicalise_blank_nodes(triples: &mut [StoredTriple]) {
    let mut map: HashMap<String, BlankNode> = HashMap::new();
    let mut next = 0usize;
    let assign = |id: &str, map: &mut HashMap<String, BlankNode>, next: &mut usize| {
        if !map.contains_key(id) {
            map.insert(id.to_owned(), BlankNode::new_unchecked(format!("b{next}")));
            *next += 1;
        }
    };
    for t in triples.iter() {
        if let NamedOrBlankNode::BlankNode(b) = &t.subject {
            assign(b.as_str(), &mut map, &mut next);
        }
        if let Term::BlankNode(b) = &t.object {
            assign(b.as_str(), &mut map, &mut next);
        }
    }
    for t in triples.iter_mut() {
        if let NamedOrBlankNode::BlankNode(b) = &t.subject
            && let Some(canonical) = map.get(b.as_str())
        {
            t.subject = NamedOrBlankNode::BlankNode(canonical.clone());
        }
        if let Term::BlankNode(b) = &t.object
            && let Some(canonical) = map.get(b.as_str())
        {
            t.object = Term::BlankNode(canonical.clone());
        }
    }
}

/// Guess a format for input that arrived without a file name.
///
/// Stdin has no extension, so something has to decide. The three shapes are
/// unmistakable at the first non-space byte, and Turtle is the fallback
/// because oxttl's Turtle parser also accepts N-Triples, which is the other
/// thing a pipeline is likely to send.
fn sniff_format(text: &str) -> (RdfFormat, &'static str) {
    let head = text.trim_start();
    if head.starts_with('{') || head.starts_with('[') {
        return (
            RdfFormat::JsonLd {
                profile: oxrdfio::JsonLdProfileSet::empty(),
            },
            "jsonld",
        );
    }
    if head.starts_with("<?xml") || head.starts_with("<rdf:RDF") || head.starts_with("<RDF") {
        return (RdfFormat::RdfXml, "rdfxml");
    }
    (RdfFormat::Turtle, "turtle")
}

/// Parse every path into one store, in the order given. The path `-` reads
/// stdin, and is cited as `-` in every finding that comes from it.
pub fn load(paths: &[Utf8PathBuf]) -> Result<Store> {
    let mut store = Store::default();
    for path in paths {
        let (format, format_name, reader): (_, _, Box<dyn BufRead>) = if path == STDIN {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("reading RDF from stdin")?;
            let (format, name) = sniff_format(&text);
            (format, name, Box::new(std::io::Cursor::new(text)))
        } else {
            let (format, name) =
                format_for(path).with_context(|| format!("unknown RDF file extension: {path}"))?;
            let file = File::open(path).with_context(|| format!("opening {path}"))?;
            (format, name, Box::new(BufReader::new(file)))
        };
        let mut parser = RdfParser::from_format(format).for_reader(reader);
        let file_index = store.files.len();
        let mut count = 0usize;
        for quad in parser.by_ref() {
            let quad = quad.with_context(|| format!("parsing {path}"))?;
            store.triples.push(StoredTriple {
                file: file_index,
                subject: quad.subject,
                predicate: quad.predicate.as_str().to_owned(),
                object: quad.object,
            });
            count += 1;
        }
        let prefixes: BTreeMap<String, String> = parser
            .prefixes()
            .map(|(p, ns)| (p.to_owned(), ns.to_owned()))
            .collect();
        let base_iri = parser.base_iri().map(str::to_owned);
        store.files.push(SourceFile {
            path: path.clone(),
            format: format_name,
            prefixes,
            base_iri,
            triple_count: count,
        });
    }
    canonicalise_blank_nodes(&mut store.triples);
    store.index();
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_matching() {
        assert!(matches_pattern("categories.ttl", "*.ttl"));
        assert!(matches_pattern("categories.ttl", "categories.ttl"));
        assert!(matches_pattern("bffo-shapes.ttl", "bffo-*.ttl"));
        assert!(!matches_pattern("categories.rdf", "*.ttl"));
        assert!(!matches_pattern("notes.ttl.bak", "*.ttl"));
        assert!(matches_pattern("anything", "*"));
    }

    #[test]
    fn iri_splitting() {
        use crate::vocab::split_iri;
        assert_eq!(
            split_iri("https://bffo.org/ontology/Format"),
            Some(("https://bffo.org/ontology/", "Format"))
        );
        assert_eq!(split_iri("https://bffo.org/ontology/"), None);
        assert_eq!(
            split_iri("http://www.w3.org/2004/02/skos/core#Concept"),
            Some(("http://www.w3.org/2004/02/skos/core#", "Concept"))
        );
    }
}
