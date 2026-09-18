//! IRI constants for the vocabularies `iyo` reads.
//!
//! Only IRIs that the code compares against live here. Anything a *profile*
//! decides (which predicate carries a label, a definition, a note) belongs in
//! `profiles/*.toml`, not in this file, so that adding a profile needs no Rust.

macro_rules! ns {
    ($base:literal; $($name:ident = $local:literal;)*) => {
        $(pub const $name: &str = concat!($base, $local);)*
    };
}

pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
pub const OWL: &str = "http://www.w3.org/2002/07/owl#";
pub const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
pub const DCTERMS: &str = "http://purl.org/dc/terms/";
pub const DC11: &str = "http://purl.org/dc/elements/1.1/";
pub const SH: &str = "http://www.w3.org/ns/shacl#";
pub const VANN: &str = "http://purl.org/vocab/vann/";
pub const ADMS: &str = "http://www.w3.org/ns/adms#";
pub const FOAF: &str = "http://xmlns.com/foaf/0.1/";
pub const SCHEMA: &str = "https://schema.org/";
pub const SCHEMA_HTTP: &str = "http://schema.org/";
pub const DCAT: &str = "http://www.w3.org/ns/dcat#";
pub const DCAM: &str = "http://purl.org/dc/dcam/";
pub const PROV: &str = "http://www.w3.org/ns/prov#";
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
pub const VS: &str = "http://www.w3.org/2003/06/sw-vocab-status/ns#";

ns!("http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    RDF_TYPE = "type";
    RDF_PROPERTY = "Property";
    RDF_LANG_STRING = "langString";
    RDF_FIRST = "first";
    RDF_REST = "rest";
    RDF_NIL = "nil";
);

ns!("http://www.w3.org/2000/01/rdf-schema#";
    RDFS_CLASS = "Class";
    RDFS_DATATYPE = "Datatype";
    RDFS_LABEL = "label";
    RDFS_COMMENT = "comment";
    RDFS_IS_DEFINED_BY = "isDefinedBy";
    RDFS_SEE_ALSO = "seeAlso";
    RDFS_SUB_CLASS_OF = "subClassOf";
    RDFS_SUB_PROPERTY_OF = "subPropertyOf";
    RDFS_DOMAIN = "domain";
    RDFS_RANGE = "range";
);

ns!("http://www.w3.org/2002/07/owl#";
    OWL_ONTOLOGY = "Ontology";
    OWL_CLASS = "Class";
    OWL_OBJECT_PROPERTY = "ObjectProperty";
    OWL_DATATYPE_PROPERTY = "DatatypeProperty";
    OWL_ANNOTATION_PROPERTY = "AnnotationProperty";
    OWL_NAMED_INDIVIDUAL = "NamedIndividual";
    OWL_VERSION_IRI = "versionIRI";
    OWL_VERSION_INFO = "versionInfo";
    OWL_PRIOR_VERSION = "priorVersion";
    OWL_BACKWARD_COMPATIBLE_WITH = "backwardCompatibleWith";
    OWL_IMPORTS = "imports";
    OWL_DEPRECATED = "deprecated";
    OWL_INVERSE_OF = "inverseOf";
    OWL_EQUIVALENT_CLASS = "equivalentClass";
    OWL_EQUIVALENT_PROPERTY = "equivalentProperty";
    OWL_DISJOINT_WITH = "disjointWith";
    OWL_SYMMETRIC_PROPERTY = "SymmetricProperty";
    OWL_ASYMMETRIC_PROPERTY = "AsymmetricProperty";
    OWL_TRANSITIVE_PROPERTY = "TransitiveProperty";
    OWL_FUNCTIONAL_PROPERTY = "FunctionalProperty";
    OWL_INVERSE_FUNCTIONAL_PROPERTY = "InverseFunctionalProperty";
    OWL_REFLEXIVE_PROPERTY = "ReflexiveProperty";
    OWL_IRREFLEXIVE_PROPERTY = "IrreflexiveProperty";
);

ns!("http://www.w3.org/2004/02/skos/core#";
    SKOS_CONCEPT = "Concept";
    SKOS_CONCEPT_SCHEME = "ConceptScheme";
    SKOS_COLLECTION = "Collection";
    SKOS_PREF_LABEL = "prefLabel";
    SKOS_ALT_LABEL = "altLabel";
    SKOS_HIDDEN_LABEL = "hiddenLabel";
    SKOS_DEFINITION = "definition";
    SKOS_NOTATION = "notation";
    SKOS_SCOPE_NOTE = "scopeNote";
    SKOS_EXAMPLE = "example";
    SKOS_HISTORY_NOTE = "historyNote";
    SKOS_CHANGE_NOTE = "changeNote";
    SKOS_EDITORIAL_NOTE = "editorialNote";
    SKOS_IN_SCHEME = "inScheme";
    SKOS_TOP_CONCEPT_OF = "topConceptOf";
    SKOS_HAS_TOP_CONCEPT = "hasTopConcept";
    SKOS_BROADER = "broader";
    SKOS_NARROWER = "narrower";
    SKOS_RELATED = "related";
    SKOS_EXACT_MATCH = "exactMatch";
    SKOS_CLOSE_MATCH = "closeMatch";
    SKOS_BROAD_MATCH = "broadMatch";
    SKOS_NARROW_MATCH = "narrowMatch";
    SKOS_RELATED_MATCH = "relatedMatch";
);

ns!("http://purl.org/dc/terms/";
    DCTERMS_TITLE = "title";
    DCTERMS_DESCRIPTION = "description";
    DCTERMS_ABSTRACT = "abstract";
    DCTERMS_CREATOR = "creator";
    DCTERMS_PUBLISHER = "publisher";
    DCTERMS_CONTRIBUTOR = "contributor";
    DCTERMS_CREATED = "created";
    DCTERMS_MODIFIED = "modified";
    DCTERMS_ISSUED = "issued";
    DCTERMS_LICENSE = "license";
    DCTERMS_RIGHTS = "rights";
    DCTERMS_HAS_PART = "hasPart";
    DCTERMS_IS_PART_OF = "isPartOf";
    DCTERMS_IS_REPLACED_BY = "isReplacedBy";
    DCTERMS_REPLACES = "replaces";
    DCTERMS_IDENTIFIER = "identifier";
    DCTERMS_BIBLIOGRAPHIC_CITATION = "bibliographicCitation";
    DCTERMS_SOURCE = "source";
    DCTERMS_TYPE = "type";
    DCTERMS_SUBJECT = "subject";
    DCTERMS_CONFORMS_TO = "conformsTo";
    DCTERMS_REFERENCES = "references";
    DCTERMS_HAS_VERSION = "hasVersion";
    DCTERMS_IS_VERSION_OF = "isVersionOf";
);

ns!("http://www.w3.org/ns/shacl#";
    SH_NODE_SHAPE = "NodeShape";
    SH_PROPERTY_SHAPE = "PropertyShape";
    SH_SHAPES_GRAPH = "ShapesGraph";
    SH_PROPERTY = "property";
    SH_PATH = "path";
    SH_TARGET_CLASS = "targetClass";
    SH_TARGET_SUBJECTS_OF = "targetSubjectsOf";
    SH_TARGET_OBJECTS_OF = "targetObjectsOf";
    SH_NAME = "name";
    SH_DESCRIPTION = "description";
    SH_MIN_COUNT = "minCount";
    SH_MAX_COUNT = "maxCount";
    SH_DATATYPE = "datatype";
    SH_CLASS = "class";
    SH_NODE_KIND = "nodeKind";
    SH_IN = "in";
    SH_HAS_VALUE = "hasValue";
    SH_NODE = "node";
    SH_SEVERITY = "severity";
    SH_OR = "or";
    SH_AND = "and";
    SH_XONE = "xone";
    SH_NOT = "not";
    SH_INVERSE_PATH = "inversePath";
    SH_PATTERN = "pattern";
    SH_MIN_LENGTH = "minLength";
    SH_MAX_LENGTH = "maxLength";
    SH_MIN_INCLUSIVE = "minInclusive";
    SH_MAX_INCLUSIVE = "maxInclusive";
    SH_CLOSED = "closed";
    SH_TARGET_NODE = "targetNode";
    SH_DEACTIVATED = "deactivated";
    SH_ORDER = "order";
    SH_GROUP = "group";
    SH_DEFAULT_VALUE = "defaultValue";
);

ns!("http://purl.org/vocab/vann/";
    VANN_PREFERRED_NAMESPACE_PREFIX = "preferredNamespacePrefix";
    VANN_PREFERRED_NAMESPACE_URI = "preferredNamespaceUri";
    VANN_EXAMPLE = "example";
    VANN_USAGE_NOTE = "usageNote";
);

ns!("http://www.w3.org/ns/adms#";
    ADMS_STATUS = "status";
);

ns!("http://xmlns.com/foaf/0.1/";
    FOAF_NAME = "name";
    FOAF_PERSON = "Person";
    FOAF_ORGANIZATION = "Organization";
    FOAF_AGENT = "Agent";
    FOAF_PAGE = "page";
    FOAF_HOMEPAGE = "homepage";
);

ns!("http://purl.org/dc/dcam/";
    DCAM_VOCABULARY_ENCODING_SCHEME = "VocabularyEncodingScheme";
    DCAM_RANGE_INCLUDES = "rangeIncludes";
    DCAM_DOMAIN_INCLUDES = "domainIncludes";
    DCAM_MEMBER_OF = "memberOf";
);

/// The property characteristics `iyo` recognises, as (IRI, display name).
pub const CHARACTERISTICS: &[(&str, &str)] = &[
    (OWL_SYMMETRIC_PROPERTY, "symmetric"),
    (OWL_ASYMMETRIC_PROPERTY, "asymmetric"),
    (OWL_TRANSITIVE_PROPERTY, "transitive"),
    (OWL_FUNCTIONAL_PROPERTY, "functional"),
    (OWL_INVERSE_FUNCTIONAL_PROPERTY, "inverse functional"),
    (OWL_REFLEXIVE_PROPERTY, "reflexive"),
    (OWL_IRREFLEXIVE_PROPERTY, "irreflexive"),
];

/// Well-known prefixes, used to render CURIEs for foreign terms when the input
/// files did not declare a prefix for them.
pub const WELL_KNOWN_PREFIXES: &[(&str, &str)] = &[
    ("rdf", RDF),
    ("rdfs", RDFS),
    ("owl", OWL),
    ("skos", SKOS),
    ("dcterms", DCTERMS),
    ("dc", DC11),
    ("sh", SH),
    ("vann", VANN),
    ("adms", ADMS),
    ("foaf", FOAF),
    ("schema", SCHEMA),
    ("dcat", DCAT),
    ("dcam", DCAM),
    ("prov", PROV),
    ("xsd", XSD),
    ("vs", VS),
];

/// Split an IRI into (namespace, local name) at the last `#`, `/` or `:`.
///
/// Returns `None` when the IRI has no usable local part, which is the case for
/// namespace IRIs themselves (`https://bffo.org/ontology/`).
pub fn split_iri(iri: &str) -> Option<(&str, &str)> {
    let cut = iri.rfind(['#', '/'])?;
    if cut + 1 >= iri.len() {
        return None; // ends with the delimiter: this is a namespace, not a term
    }
    Some((&iri[..=cut], &iri[cut + 1..]))
}

/// The local name of an IRI, or the whole IRI when it has none.
pub fn local_name(iri: &str) -> &str {
    split_iri(iri).map(|(_, l)| l).unwrap_or(iri)
}
