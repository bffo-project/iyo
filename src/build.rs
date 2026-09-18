//! Stages 2 to 4: partition the store into documents, namespaces and terms,
//! then read each term through its document's profile.
//!
//! Two rules from the requirements drive the shape of this file:
//!
//! * a term belongs to the document that *defines* it, found by
//!   `rdfs:isDefinedBy`, then `skos:inScheme`, then the source file, then the
//!   longest namespace prefix, so `bffo:FormatShape` lands on the
//!   shapes page even though its IRI is in the ontology namespace;
//! * a term is foreign when its namespace is not minted by this release,
//!   regardless of its `rdf:type`, so `dcterms:replaces` typed
//!   `owl:ObjectProperty` is still reused, not defined here.

use crate::load::{Store, term_iri};
use crate::model::*;
use crate::profile::{Registry, Signals};
use crate::vocab as v;
use anyhow::Result;
use oxrdf::Term as RdfTerm;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Convert a parsed object into a model node.
fn node_of(term: &RdfTerm) -> Node {
    match term {
        RdfTerm::NamedNode(n) => Node::Iri {
            iri: n.as_str().to_owned(),
        },
        RdfTerm::BlankNode(b) => Node::Blank {
            id: b.as_str().to_owned(),
        },
        RdfTerm::Literal(l) => Node::Literal {
            value: l.value().to_owned(),
            lang: l.language().map(str::to_owned),
            datatype: {
                let dt = l.datatype().as_str().to_owned();
                if dt == format!("{}string", v::XSD) || dt == v::RDF_LANG_STRING {
                    None
                } else {
                    Some(dt)
                }
            },
        },
    }
}

/// The lexical value of a literal, or the IRI of a named node.
fn scalar(term: &RdfTerm) -> Option<String> {
    match term {
        RdfTerm::NamedNode(n) => Some(n.as_str().to_owned()),
        RdfTerm::Literal(l) => Some(l.value().to_owned()),
        _ => None,
    }
}

fn collect_lang(
    store: &Store,
    subject: &str,
    predicates: &[String],
    consumed: &mut HashSet<String>,
) -> Vec<LangString> {
    let mut out = Vec::new();
    for p in predicates {
        consumed.insert(p.clone());
        for o in store.objects(subject, p) {
            if let RdfTerm::Literal(l) = o {
                out.push(LangString {
                    value: l.value().to_owned(),
                    lang: l.language().map(str::to_owned),
                    source: p.clone(),
                });
            }
        }
    }
    out
}

fn collect_iris(
    store: &Store,
    subject: &str,
    predicates: &[&str],
    consumed: &mut HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for p in predicates {
        consumed.insert((*p).to_owned());
        out.extend(store.iri_objects(subject, p));
    }
    out.sort();
    out.dedup();
    out
}

fn first_scalar(
    store: &Store,
    subject: &str,
    predicates: &[&str],
    consumed: &mut HashSet<String>,
) -> Option<String> {
    let mut found = None;
    for p in predicates {
        consumed.insert((*p).to_owned());
        if found.is_none() {
            found = store.object(subject, p).and_then(scalar);
        }
    }
    found
}

fn agents(
    store: &Store,
    subject: &str,
    predicate: &str,
    consumed: &mut HashSet<String>,
) -> Vec<Agent> {
    consumed.insert(predicate.to_owned());
    let mut out = Vec::new();
    for o in store.objects(subject, predicate) {
        match o {
            RdfTerm::NamedNode(n) => out.push(Agent {
                iri: Some(n.as_str().to_owned()),
                name: store
                    .object(n.as_str(), v::FOAF_NAME)
                    .and_then(scalar)
                    .or_else(|| store.object(n.as_str(), v::RDFS_LABEL).and_then(scalar)),
                kind: store.types(n.as_str()).first().cloned(),
            }),
            RdfTerm::BlankNode(b) => {
                let key = format!("_:{}", b.as_str());
                out.push(Agent {
                    iri: None,
                    name: store
                        .object(&key, v::FOAF_NAME)
                        .and_then(scalar)
                        .or_else(|| store.object(&key, v::RDFS_LABEL).and_then(scalar)),
                    kind: store.types(&key).first().cloned(),
                });
            }
            RdfTerm::Literal(l) => out.push(Agent {
                iri: None,
                name: Some(l.value().to_owned()),
                kind: None,
            }),
        }
    }
    out
}

fn kind_from_types(types: &[String], has_domain_or_range: bool) -> TermKind {
    let has = |iri: &str| types.iter().any(|t| t == iri);
    if has(v::SH_NODE_SHAPE) {
        return TermKind::NodeShape;
    }
    if has(v::SH_PROPERTY_SHAPE) {
        return TermKind::PropertyShape;
    }
    if has(v::SKOS_CONCEPT) {
        return TermKind::Concept;
    }
    if has(v::SKOS_COLLECTION) {
        return TermKind::Collection;
    }
    if has(v::OWL_CLASS) || has(v::RDFS_CLASS) {
        return TermKind::Class;
    }
    if has(v::OWL_OBJECT_PROPERTY) {
        return TermKind::ObjectProperty;
    }
    if has(v::OWL_DATATYPE_PROPERTY) {
        return TermKind::DatatypeProperty;
    }
    if has(v::OWL_ANNOTATION_PROPERTY) {
        return TermKind::AnnotationProperty;
    }
    if has(v::RDF_PROPERTY) {
        return TermKind::RdfProperty;
    }
    if has(v::RDFS_DATATYPE) {
        return TermKind::Datatype;
    }
    if has(v::DCAM_VOCABULARY_ENCODING_SCHEME) {
        return TermKind::EncodingScheme;
    }
    if has(v::OWL_NAMED_INDIVIDUAL) {
        return TermKind::Individual;
    }
    if has_domain_or_range {
        return TermKind::RdfProperty;
    }
    if types.is_empty() {
        TermKind::Other
    } else {
        TermKind::Individual
    }
}

/// The longest minted namespace that is a prefix of `iri`.
fn namespace_of<'a>(iri: &str, minted: &'a BTreeSet<String>) -> Option<&'a String> {
    minted
        .iter()
        .filter(|ns| iri.starts_with(ns.as_str()) && iri.len() > ns.len())
        .max_by_key(|ns| ns.len())
}

fn prefix_for(namespace: &str, prefixes: &BTreeMap<String, String>) -> Option<String> {
    if let Some((p, _)) = prefixes.iter().find(|(_, ns)| ns.as_str() == namespace) {
        return Some(p.clone());
    }
    v::WELL_KNOWN_PREFIXES
        .iter()
        .find(|(_, ns)| *ns == namespace)
        .map(|(p, _)| (*p).to_owned())
}

/// Build the model from a loaded store.
pub fn build(store: &Store, registry: &Registry) -> Result<Release> {
    // ---- documents -------------------------------------------------------
    let mut document_iris: BTreeSet<String> = BTreeSet::new();
    for class in [v::OWL_ONTOLOGY, v::SKOS_CONCEPT_SCHEME] {
        for s in store.subjects_with(v::RDF_TYPE, class) {
            if !s.starts_with("_:") {
                document_iris.insert(s);
            }
        }
    }

    // ---- minted namespaces ----------------------------------------------
    let mut minted: BTreeSet<String> = BTreeSet::new();
    for d in &document_iris {
        if d.ends_with('/') || d.ends_with('#') {
            minted.insert(d.clone());
        }
    }
    for s in store.subjects_of(v::VANN_PREFERRED_NAMESPACE_URI) {
        for o in store.objects(&s, v::VANN_PREFERRED_NAMESPACE_URI) {
            if let Some(value) = scalar(o) {
                minted.insert(value);
            }
        }
    }

    // ---- per-file signals for profile detection --------------------------
    let file_count = store.files.len();
    let mut file_types: Vec<BTreeSet<String>> = vec![BTreeSet::new(); file_count];
    let mut file_predicates: Vec<BTreeSet<String>> = vec![BTreeSet::new(); file_count];
    for t in &store.triples {
        file_predicates[t.file].insert(t.predicate.clone());
        if t.predicate == v::RDF_TYPE
            && let Some(iri) = term_iri(&t.object)
        {
            file_types[t.file].insert(iri.to_owned());
        }
    }

    let prefixes = store.prefixes();

    // ---- documents, with header and profile ------------------------------
    let mut documents: Vec<Document> = Vec::new();
    for iri in &document_iris {
        let source_file = store.file_of(iri).unwrap_or(0);
        let types = store.types(iri);
        let ft: Vec<String> = file_types[source_file].iter().cloned().collect();
        let fp: Vec<String> = file_predicates[source_file].iter().cloned().collect();
        let profile = registry.detect(&Signals {
            document_types: &types,
            file_types: &ft,
            file_predicates: &fp,
        });
        let kind = if profile.id == "shacl" {
            DocumentKind::Shapes
        } else if types.iter().any(|t| t == v::SKOS_CONCEPT_SCHEME) {
            DocumentKind::Scheme
        } else if types.iter().any(|t| t == v::OWL_ONTOLOGY) {
            DocumentKind::Ontology
        } else {
            DocumentKind::Document
        };

        let mut consumed: HashSet<String> = HashSet::new();
        let ann = &profile.annotations;
        let title_predicates = if ann.document_title.is_empty() {
            &ann.label
        } else {
            &ann.document_title
        };
        let description_predicates = if ann.document_description.is_empty() {
            &ann.definition
        } else {
            &ann.document_description
        };
        let header = Header {
            title: collect_lang(store, iri, title_predicates, &mut consumed),
            description: collect_lang(store, iri, description_predicates, &mut consumed),
            comment: collect_lang(store, iri, &[v::RDFS_COMMENT.to_owned()], &mut consumed),
            abstract_: collect_lang(store, iri, &[v::DCTERMS_ABSTRACT.to_owned()], &mut consumed),
            creators: agents(store, iri, v::DCTERMS_CREATOR, &mut consumed),
            publishers: agents(store, iri, v::DCTERMS_PUBLISHER, &mut consumed),
            contributors: agents(store, iri, v::DCTERMS_CONTRIBUTOR, &mut consumed),
            created: first_scalar(store, iri, &[v::DCTERMS_CREATED], &mut consumed),
            modified: first_scalar(store, iri, &[v::DCTERMS_MODIFIED], &mut consumed),
            issued: first_scalar(store, iri, &[v::DCTERMS_ISSUED], &mut consumed),
            license: first_scalar(store, iri, &[v::DCTERMS_LICENSE], &mut consumed),
            rights: collect_lang(store, iri, &[v::DCTERMS_RIGHTS.to_owned()], &mut consumed),
            version_info: first_scalar(store, iri, &[v::OWL_VERSION_INFO], &mut consumed),
            version_iri: first_scalar(store, iri, &[v::OWL_VERSION_IRI], &mut consumed),
            prior_version: first_scalar(store, iri, &[v::OWL_PRIOR_VERSION], &mut consumed),
            status: first_scalar(store, iri, &[v::ADMS_STATUS], &mut consumed),
            prefix: first_scalar(
                store,
                iri,
                &[v::VANN_PREFERRED_NAMESPACE_PREFIX],
                &mut consumed,
            ),
            namespace_uri: first_scalar(
                store,
                iri,
                &[v::VANN_PREFERRED_NAMESPACE_URI],
                &mut consumed,
            ),
            citation: first_scalar(
                store,
                iri,
                &[v::DCTERMS_BIBLIOGRAPHIC_CITATION],
                &mut consumed,
            ),
            identifier: first_scalar(store, iri, &[v::DCTERMS_IDENTIFIER], &mut consumed),
            has_part: collect_iris(store, iri, &[v::DCTERMS_HAS_PART], &mut consumed),
            is_part_of: collect_iris(store, iri, &[v::DCTERMS_IS_PART_OF], &mut consumed),
            see_also: collect_iris(store, iri, &[v::RDFS_SEE_ALSO], &mut consumed),
            imports: collect_iris(store, iri, &[v::OWL_IMPORTS], &mut consumed),
            source: collect_iris(store, iri, &[v::DCTERMS_SOURCE], &mut consumed),
        };

        documents.push(Document {
            iri: iri.clone(),
            kind,
            source_file,
            profile: profile.id.clone(),
            header,
            terms: Vec::new(),
            foreign_terms: Vec::new(),
        });
    }
    documents.sort_by(|a, b| a.iri.cmp(&b.iri));

    // ---- terms -----------------------------------------------------------
    let mut terms: Vec<Term> = Vec::new();
    for subject in store.named_subjects() {
        if document_iris.contains(&subject) {
            continue;
        }
        let source_file = store.file_of(&subject).unwrap_or(0);
        let ns_minted = namespace_of(&subject, &minted);
        let foreign = ns_minted.is_none();
        let namespace = ns_minted.cloned().unwrap_or_else(|| {
            v::split_iri(&subject)
                .map(|(ns, _)| ns.to_owned())
                .unwrap_or_else(|| subject.clone())
        });
        let local_name = subject
            .strip_prefix(namespace.as_str())
            .unwrap_or_else(|| v::local_name(&subject))
            .to_owned();

        // Which document defines it.
        let is_defined_by = store
            .iri_objects(&subject, v::RDFS_IS_DEFINED_BY)
            .into_iter()
            .find(|d| document_iris.contains(d));
        let in_scheme = store
            .iri_objects(&subject, v::SKOS_IN_SCHEME)
            .into_iter()
            .find(|d| document_iris.contains(d));
        let by_file: Option<String> = {
            let in_file: Vec<&Document> = documents
                .iter()
                .filter(|d| d.source_file == source_file)
                .collect();
            match in_file.len() {
                1 => Some(in_file[0].iri.clone()),
                0 => None,
                _ => in_file
                    .iter()
                    .filter(|d| subject.starts_with(&d.iri))
                    .max_by_key(|d| d.iri.len())
                    .map(|d| d.iri.clone()),
            }
        };
        let by_namespace = documents
            .iter()
            .filter(|d| subject.starts_with(&d.iri))
            .max_by_key(|d| d.iri.len())
            .map(|d| d.iri.clone());
        let defined_in = is_defined_by
            .clone()
            .or(in_scheme.clone())
            .or(by_file)
            .or(by_namespace);

        let profile = defined_in
            .as_ref()
            .and_then(|d| documents.iter().find(|doc| &doc.iri == d))
            .and_then(|d| registry.get(&d.profile))
            .or_else(|| registry.get("generic"))
            .expect("generic profile present");
        let ann = &profile.annotations;

        let mut consumed: HashSet<String> = HashSet::new();
        consumed.insert(v::RDF_TYPE.to_owned());
        consumed.insert(v::RDFS_IS_DEFINED_BY.to_owned());

        let types = store.types(&subject);
        let labels = collect_lang(store, &subject, &ann.label, &mut consumed);
        let alt_labels = collect_lang(store, &subject, &ann.alt_label, &mut consumed);
        let definitions = collect_lang(store, &subject, &ann.definition, &mut consumed);
        let comments = collect_lang(store, &subject, &ann.comment, &mut consumed);
        let notes = collect_lang(store, &subject, &ann.note, &mut consumed);
        let examples = collect_lang(store, &subject, &ann.example, &mut consumed);

        let super_terms = collect_iris(
            store,
            &subject,
            &[v::RDFS_SUB_CLASS_OF, v::RDFS_SUB_PROPERTY_OF],
            &mut consumed,
        );
        let equivalent = collect_iris(
            store,
            &subject,
            &[v::OWL_EQUIVALENT_CLASS, v::OWL_EQUIVALENT_PROPERTY],
            &mut consumed,
        );
        let disjoint_with = collect_iris(store, &subject, &[v::OWL_DISJOINT_WITH], &mut consumed);
        let see_also = collect_iris(store, &subject, &[v::RDFS_SEE_ALSO], &mut consumed);
        let replaced_by =
            collect_iris(store, &subject, &[v::DCTERMS_IS_REPLACED_BY], &mut consumed);

        let property = PropertyFacts {
            domain: collect_iris(store, &subject, &[v::RDFS_DOMAIN], &mut consumed),
            range: collect_iris(store, &subject, &[v::RDFS_RANGE], &mut consumed),
            domain_includes: collect_iris(
                store,
                &subject,
                &[v::DCAM_DOMAIN_INCLUDES],
                &mut consumed,
            ),
            range_includes: collect_iris(store, &subject, &[v::DCAM_RANGE_INCLUDES], &mut consumed),
            inverse_of: collect_iris(store, &subject, &[v::OWL_INVERSE_OF], &mut consumed),
            characteristics: v::CHARACTERISTICS
                .iter()
                .filter(|(iri, _)| types.iter().any(|t| t == iri))
                .map(|(_, name)| (*name).to_owned())
                .collect(),
        };

        let concept = ConceptFacts {
            in_scheme: collect_iris(store, &subject, &[v::SKOS_IN_SCHEME], &mut consumed),
            top_concept_of: collect_iris(store, &subject, &[v::SKOS_TOP_CONCEPT_OF], &mut consumed),
            broader: collect_iris(store, &subject, &[v::SKOS_BROADER], &mut consumed),
            narrower: collect_iris(store, &subject, &[v::SKOS_NARROWER], &mut consumed),
            related: collect_iris(store, &subject, &[v::SKOS_RELATED], &mut consumed),
            notation: first_scalar(store, &subject, &[v::SKOS_NOTATION], &mut consumed),
        };

        let mut mappings = Vec::new();
        for rel in [
            v::SKOS_EXACT_MATCH,
            v::SKOS_CLOSE_MATCH,
            v::SKOS_BROAD_MATCH,
            v::SKOS_NARROW_MATCH,
            v::SKOS_RELATED_MATCH,
        ] {
            consumed.insert(rel.to_owned());
            for iri in store.iri_objects(&subject, rel) {
                mappings.push(Mapping {
                    relation: rel.to_owned(),
                    iri,
                });
            }
        }

        let deprecated = {
            let mut flag = false;
            for p in &ann.deprecated {
                consumed.insert(p.clone());
                if let Some(o) = store.object(&subject, p)
                    && scalar(o).as_deref() == Some("true")
                {
                    flag = true;
                }
            }
            flag
        };
        let status = {
            let list: Vec<&str> = ann.status.iter().map(String::as_str).collect();
            first_scalar(store, &subject, &list, &mut consumed)
        };

        // The structural predicates of a shape are rendered as a constraint
        // table, so leaving them in the residue would list 37 blank node
        // references under "further statements" and say nothing.
        for p in [
            v::SH_PROPERTY,
            v::SH_TARGET_CLASS,
            v::SH_TARGET_SUBJECTS_OF,
            v::SH_TARGET_OBJECTS_OF,
            v::SH_TARGET_NODE,
            v::SH_CLOSED,
        ] {
            consumed.insert(p.to_owned());
        }

        let residue: Vec<Statement> = store
            .about(&subject)
            .filter(|t| !consumed.contains(&t.predicate))
            .map(|t| Statement {
                predicate: t.predicate.clone(),
                object: node_of(&t.object),
            })
            .collect();

        let prefix = prefix_for(&namespace, &prefixes);
        let curie = prefix.as_ref().map(|p| format!("{p}:{local_name}"));
        let anchor = if foreign {
            match &prefix {
                Some(p) => format!("{p}_{local_name}"),
                None => format!("ext_{local_name}"),
            }
        } else {
            local_name.clone()
        };
        let has_domain_or_range = !property.domain.is_empty() || !property.range.is_empty();

        terms.push(Term {
            iri: subject.clone(),
            local_name,
            curie,
            namespace,
            defined_in,
            source_file,
            foreign,
            kind: kind_from_types(&types, has_domain_or_range),
            types,
            labels,
            alt_labels,
            definitions,
            comments,
            notes,
            examples,
            see_also,
            super_terms,
            sub_terms: Vec::new(),
            equivalent,
            disjoint_with,
            mappings,
            property,
            concept,
            deprecated,
            replaced_by,
            status,
            is_defined_by,
            residue,
            anchor,
        });
    }
    terms.sort_by(|a, b| a.iri.cmp(&b.iri));

    // Invert the hierarchies now that every term is known.
    let known: BTreeSet<String> = terms.iter().map(|t| t.iri.clone()).collect();
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut narrower: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in &terms {
        for parent in &t.super_terms {
            if known.contains(parent) {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(t.iri.clone());
            }
        }
        for parent in &t.concept.broader {
            if known.contains(parent) {
                narrower
                    .entry(parent.clone())
                    .or_default()
                    .push(t.iri.clone());
            }
        }
    }
    for t in &mut terms {
        if let Some(mut c) = children.remove(&t.iri) {
            c.sort();
            c.dedup();
            t.sub_terms = c;
        }
        if let Some(mut n) = narrower.remove(&t.iri) {
            n.sort();
            n.dedup();
            for iri in n {
                if !t.concept.narrower.contains(&iri) {
                    t.concept.narrower.push(iri);
                }
            }
            t.concept.narrower.sort();
        }
    }

    // ---- attach terms to documents ---------------------------------------
    for doc in &mut documents {
        for t in &terms {
            if t.defined_in.as_deref() == Some(doc.iri.as_str()) {
                if t.foreign {
                    doc.foreign_terms.push(t.iri.clone());
                } else {
                    doc.terms.push(t.iri.clone());
                }
            }
        }
        doc.terms.sort();
        doc.foreign_terms.sort();
    }

    // ---- namespaces ------------------------------------------------------
    let mut namespaces: Vec<Namespace> = Vec::new();
    for iri in &minted {
        let document = documents
            .iter()
            .find(|d| &d.iri == iri)
            .map(|d| d.iri.clone());
        // Any document IRI nested under this namespace is a reserved segment,
        // not a term: /ontology/shapes/ under /ontology/.
        let mut reserved: Vec<String> = document_iris
            .iter()
            .filter(|d| d.starts_with(iri.as_str()) && d.as_str() != iri.as_str())
            .filter_map(|d| {
                d.strip_prefix(iri.as_str())
                    .map(|rest| rest.trim_end_matches('/').to_owned())
            })
            .filter(|rest| !rest.is_empty() && !rest.contains('/'))
            .collect();
        // A version IRI under the namespace is reserved too.
        for doc in &documents {
            if let Some(vi) = &doc.header.version_iri
                && vi.starts_with(iri.as_str())
            {
                let rest = vi
                    .strip_prefix(iri.as_str())
                    .unwrap_or_default()
                    .trim_end_matches('/');
                if !rest.is_empty() && !rest.contains('/') {
                    reserved.push(rest.to_owned());
                }
            }
        }
        reserved.sort();
        reserved.dedup();
        namespaces.push(Namespace {
            iri: iri.clone(),
            prefix: prefix_for(iri, &prefixes),
            document,
            reserved,
            term_count: terms
                .iter()
                .filter(|t| !t.foreign && &t.namespace == iri)
                .count(),
        });
    }
    namespaces.sort_by(|a, b| a.iri.cmp(&b.iri));

    // ---- stats -----------------------------------------------------------
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for t in terms.iter().filter(|t| !t.foreign) {
        *by_kind.entry(t.kind.label().to_owned()).or_default() += 1;
    }
    let stats = Stats {
        files: store.files.len(),
        triples: store.triples.len(),
        documents: documents.len(),
        namespaces: namespaces.len(),
        terms_local: terms.iter().filter(|t| !t.foreign).count(),
        terms_foreign: terms.iter().filter(|t| t.foreign).count(),
        by_kind,
    };

    let files = store
        .files
        .iter()
        .map(|f| FileInfo {
            path: f.path.to_string(),
            format: f.format.to_owned(),
            triples: f.triple_count,
            prefixes: f.prefixes.clone(),
            base_iri: f.base_iri.clone(),
        })
        .collect();

    // Shapes last: joining `sh:targetSubjectsOf` to a class needs the
    // `rdfs:domain` of the targeted property, which is only known once every
    // term has been read.
    let domains: BTreeMap<String, Vec<String>> = terms
        .iter()
        .filter(|t| !t.property.domain.is_empty())
        .map(|t| (t.iri.clone(), t.property.domain.clone()))
        .collect();
    let shapes = crate::shape::Shapes::extract(store, &domains);

    Ok(Release {
        schema_version: SCHEMA_VERSION,
        generator: Generator::default(),
        files,
        namespaces,
        documents,
        terms,
        prefixes,
        shapes,
        stats,
    })
}
