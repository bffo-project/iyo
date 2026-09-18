//! The intermediate model.
//!
//! Everything downstream reads only this: renderers, checks, the manifest and
//! any future plugin. Nothing here mentions RDF parsing, and templates never
//! see triples.
//!
//! The JSON serialisation of `Release` is a published contract, so it carries a
//! `schema_version` and changes additively within a major version.

use serde::Serialize;
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: &str = "0.1";

/// A node value kept verbatim from the RDF, for fields the model does not
/// interpret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Iri {
        iri: String,
    },
    Literal {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        lang: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        datatype: Option<String>,
    },
    Blank {
        id: String,
    },
}

impl Node {
    pub fn as_iri(&self) -> Option<&str> {
        match self {
            Node::Iri { iri } => Some(iri),
            _ => None,
        }
    }

    pub fn as_value(&self) -> Option<&str> {
        match self {
            Node::Literal { value, .. } => Some(value),
            Node::Iri { iri } => Some(iri),
            Node::Blank { .. } => None,
        }
    }
}

/// A language-tagged value, with the predicate it came from so that a page can
/// say where a fact is from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LangString {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub source: String,
}

/// A creator, publisher or contributor, given as an IRI, a blank node with a
/// name, or a plain literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// A predicate and object kept verbatim because the model has no field for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Statement {
    pub predicate: String,
    pub object: Node,
}

/// A typed mapping to a term in another vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mapping {
    /// The mapping predicate, for example `skos:exactMatch`.
    pub relation: String,
    pub iri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    /// An `owl:Ontology` that declares terms.
    Ontology,
    /// A `skos:ConceptScheme`.
    Scheme,
    /// A document whose subjects are SHACL shapes.
    Shapes,
    /// Documentation only: no terms of its own.
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TermKind {
    Class,
    ObjectProperty,
    DatatypeProperty,
    AnnotationProperty,
    /// A bare `rdf:Property`, which is how DCMI declares its terms.
    RdfProperty,
    Datatype,
    /// `dcam:VocabularyEncodingScheme`.
    EncodingScheme,
    Concept,
    Collection,
    NodeShape,
    PropertyShape,
    Individual,
    Other,
}

impl TermKind {
    pub fn label(self) -> &'static str {
        match self {
            TermKind::Class => "class",
            TermKind::ObjectProperty => "object property",
            TermKind::DatatypeProperty => "datatype property",
            TermKind::AnnotationProperty => "annotation property",
            TermKind::RdfProperty => "property",
            TermKind::Datatype => "datatype",
            TermKind::EncodingScheme => "encoding scheme",
            TermKind::Concept => "concept",
            TermKind::Collection => "collection",
            TermKind::NodeShape => "node shape",
            TermKind::PropertyShape => "property shape",
            TermKind::Individual => "individual",
            TermKind::Other => "term",
        }
    }

    /// The heading a term reference groups this kind under.
    pub fn section(self) -> &'static str {
        match self {
            TermKind::Class => "Classes",
            TermKind::ObjectProperty => "Object properties",
            TermKind::DatatypeProperty => "Datatype properties",
            TermKind::AnnotationProperty => "Annotation properties",
            TermKind::RdfProperty => "Properties",
            TermKind::Datatype => "Datatypes",
            TermKind::EncodingScheme => "Encoding schemes",
            TermKind::Concept => "Concepts",
            TermKind::Collection => "Collections",
            TermKind::NodeShape => "Node shapes",
            TermKind::PropertyShape => "Property shapes",
            TermKind::Individual => "Individuals",
            TermKind::Other => "Other terms",
        }
    }
}

/// Vocabulary-level metadata, read through the document's profile.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Header {
    pub title: Vec<LangString>,
    pub description: Vec<LangString>,
    #[serde(rename = "abstract")]
    pub abstract_: Vec<LangString>,
    /// `rdfs:comment` on the document node. Application profiles use it for a
    /// status banner, so it is kept apart from the description.
    pub comment: Vec<LangString>,
    pub creators: Vec<Agent>,
    pub publishers: Vec<Agent>,
    pub contributors: Vec<Agent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub rights: Vec<LangString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    pub has_part: Vec<String>,
    pub is_part_of: Vec<String>,
    pub see_also: Vec<String>,
    pub imports: Vec<String>,
    pub source: Vec<String>,
}

/// One `owl:Ontology` or `skos:ConceptScheme` in the release. BFFO has ten,
/// so this is a list, never a singleton.
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub iri: String,
    pub kind: DocumentKind,
    /// Index into `Release::files`.
    pub source_file: usize,
    pub profile: String,
    pub header: Header,
    /// IRIs of the local terms this document defines, sorted.
    pub terms: Vec<String>,
    /// IRIs of the foreign terms this document describes, sorted.
    pub foreign_terms: Vec<String>,
}

/// A namespace that mints term IRIs.
#[derive(Debug, Clone, Serialize)]
pub struct Namespace {
    pub iri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// The document that declares this namespace, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// Path segments under this namespace that are documents, not terms:
    /// `shapes`, a version string.
    pub reserved: Vec<String>,
    pub term_count: usize,
}

/// Facts a property carries.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PropertyFacts {
    pub domain: Vec<String>,
    pub range: Vec<String>,
    pub domain_includes: Vec<String>,
    pub range_includes: Vec<String>,
    pub inverse_of: Vec<String>,
    pub characteristics: Vec<String>,
}

/// Facts a SKOS concept carries.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConceptFacts {
    pub in_scheme: Vec<String>,
    pub top_concept_of: Vec<String>,
    pub broader: Vec<String>,
    pub narrower: Vec<String>,
    pub related: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Term {
    pub iri: String,
    pub local_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curie: Option<String>,
    /// The minted namespace this term belongs to, or the foreign namespace.
    pub namespace: String,
    /// The document that defines it, when one could be determined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defined_in: Option<String>,
    /// Index into `Release::files`.
    pub source_file: usize,
    /// True when the namespace is not minted by this release.
    pub foreign: bool,
    pub kind: TermKind,
    /// Every `rdf:type` IRI, sorted.
    pub types: Vec<String>,
    pub labels: Vec<LangString>,
    pub alt_labels: Vec<LangString>,
    pub definitions: Vec<LangString>,
    pub comments: Vec<LangString>,
    pub notes: Vec<LangString>,
    pub examples: Vec<LangString>,
    pub see_also: Vec<String>,
    pub super_terms: Vec<String>,
    pub sub_terms: Vec<String>,
    pub equivalent: Vec<String>,
    pub disjoint_with: Vec<String>,
    pub mappings: Vec<Mapping>,
    pub property: PropertyFacts,
    pub concept: ConceptFacts,
    pub deprecated: bool,
    pub replaced_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_defined_by: Option<String>,
    /// Statements with no model field, kept so nothing is silently lost.
    pub residue: Vec<Statement>,
    /// The anchor id on the namespace document: the local name for local
    /// terms, `prefix_local` for foreign ones.
    pub anchor: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub format: String,
    pub triples: usize,
    pub prefixes: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_iri: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub files: usize,
    pub triples: usize,
    pub documents: usize,
    pub namespaces: usize,
    pub terms_local: usize,
    pub terms_foreign: usize,
    pub by_kind: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Generator {
    pub name: &'static str,
    pub version: &'static str,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// The whole input set as one model.
#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub schema_version: &'static str,
    pub generator: Generator,
    pub files: Vec<FileInfo>,
    pub namespaces: Vec<Namespace>,
    pub documents: Vec<Document>,
    pub terms: Vec<Term>,
    /// The reconciled prefix map of every input file.
    pub prefixes: BTreeMap<String, String>,
    /// SHACL shapes, joined to the terms they constrain.
    pub shapes: crate::shape::Shapes,
    pub stats: Stats,
}

impl Release {
    pub fn term(&self, iri: &str) -> Option<&Term> {
        self.terms.iter().find(|t| t.iri == iri)
    }

    pub fn document(&self, iri: &str) -> Option<&Document> {
        self.documents.iter().find(|d| d.iri == iri)
    }

    /// The document that acts as the root of the release: the one no other
    /// document lists in `dcterms:hasPart`, preferring an ontology.
    pub fn root_document(&self) -> Option<&Document> {
        let parts: Vec<&String> = self
            .documents
            .iter()
            .flat_map(|d| d.header.has_part.iter())
            .collect();
        self.documents
            .iter()
            .filter(|d| !parts.contains(&&d.iri))
            .min_by_key(|d| (d.kind, d.iri.clone()))
    }

    /// Local terms only, in the order they should be listed.
    pub fn local_terms(&self) -> impl Iterator<Item = &Term> {
        self.terms.iter().filter(|t| !t.foreign)
    }
}

/// Pick the value for a page language: the exact tag, then an untagged value,
/// then whatever is first, so a vocabulary with no language tags still renders.
fn pick<'a>(values: &'a [LangString], lang: &str) -> Option<&'a LangString> {
    values
        .iter()
        .find(|v| v.lang.as_deref() == Some(lang))
        .or_else(|| values.iter().find(|v| v.lang.is_none()))
        .or_else(|| values.first())
}

impl Term {
    pub fn label(&self, lang: &str) -> Option<&LangString> {
        pick(&self.labels, lang)
    }

    pub fn definition(&self, lang: &str) -> Option<&LangString> {
        pick(&self.definitions, lang)
    }

    /// The label, or the local name when there is none.
    pub fn display(&self, lang: &str) -> &str {
        self.label(lang)
            .map(|l| l.value.as_str())
            .unwrap_or(&self.local_name)
    }

    /// A one-line definition for indexes and `llms.txt`, with newlines
    /// flattened and no truncation of meaning beyond the first sentence.
    pub fn summary(&self, lang: &str) -> Option<String> {
        let text = self.definition(lang)?.value.replace(['\n', '\r'], " ");
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(match text.find(". ") {
            Some(i) if i > 40 => text[..=i].trim().to_owned(),
            _ => text,
        })
    }
}

impl Document {
    pub fn title(&self, lang: &str) -> Option<&LangString> {
        pick(&self.header.title, lang)
    }

    pub fn description(&self, lang: &str) -> Option<&LangString> {
        pick(&self.header.description, lang)
    }

    pub fn display(&self, lang: &str) -> &str {
        self.title(lang)
            .map(|t| t.value.as_str())
            .unwrap_or(&self.iri)
    }
}

/// Render an IRI as a CURIE when a declared prefix covers it.
pub fn curie(iri: &str, prefixes: &BTreeMap<String, String>) -> Option<String> {
    let (namespace, local) = crate::vocab::split_iri(iri)?;
    if local.is_empty() {
        return None;
    }
    prefixes
        .iter()
        .find(|(_, ns)| ns.as_str() == namespace)
        .map(|(p, _)| format!("{p}:{local}"))
        .or_else(|| {
            crate::vocab::WELL_KNOWN_PREFIXES
                .iter()
                .find(|(_, ns)| *ns == namespace)
                .map(|(p, _)| format!("{p}:{local}"))
        })
}

/// A CURIE if one exists, otherwise the IRI itself.
pub fn short(iri: &str, prefixes: &BTreeMap<String, String>) -> String {
    curie(iri, prefixes).unwrap_or_else(|| iri.to_owned())
}
