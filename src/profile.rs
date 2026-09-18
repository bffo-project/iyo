//! Profiles: which predicates carry which model field, and how a document is
//! recognised.
//!
//! Profiles are TOML data compiled into the binary, not Rust. A contributor
//! adds a vocabulary style by writing a file in `profiles/`, which is the
//! extension point the design promises to people who do not write Rust.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The profiles shipped with the binary, in no particular order.
const BUILT_IN: &[(&str, &str)] = &[
    ("generic", include_str!("../profiles/generic.toml")),
    ("dcap", include_str!("../profiles/dcap.toml")),
    ("skos", include_str!("../profiles/skos.toml")),
    ("shacl", include_str!("../profiles/shacl.toml")),
    ("dcmi", include_str!("../profiles/dcmi.toml")),
];

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Detect {
    #[serde(default)]
    pub priority: i32,
    /// The document node itself must carry one of these `rdf:type` values.
    #[serde(default)]
    pub document_type: Vec<String>,
    /// Some subject in the document's file must carry one of these types.
    #[serde(default)]
    pub file_has_type: Vec<String>,
    /// Some triple in the document's file must use one of these predicates.
    #[serde(default)]
    pub file_has_predicate: Vec<String>,
}

/// Predicate lists per model field, in priority order.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Annotations {
    #[serde(default)]
    pub label: Vec<String>,
    /// Title of a document. Distinct from `label` because a vocabulary's own
    /// name and a term's label are rarely the same predicate.
    #[serde(default)]
    pub document_title: Vec<String>,
    /// Description of a document. Distinct from `definition` because an
    /// application profile often carries a status banner in `rdfs:comment`,
    /// which must not become the vocabulary's description.
    #[serde(default)]
    pub document_description: Vec<String>,
    #[serde(default)]
    pub definition: Vec<String>,
    #[serde(default)]
    pub comment: Vec<String>,
    #[serde(default)]
    pub alt_label: Vec<String>,
    #[serde(default)]
    pub note: Vec<String>,
    #[serde(default)]
    pub example: Vec<String>,
    #[serde(default)]
    pub see_also: Vec<String>,
    #[serde(default)]
    pub status: Vec<String>,
    #[serde(default)]
    pub deprecated: Vec<String>,
    #[serde(default)]
    pub replaced_by: Vec<String>,
    #[serde(default)]
    pub created: Vec<String>,
    #[serde(default)]
    pub modified: Vec<String>,
    #[serde(default)]
    pub issued: Vec<String>,
}

impl Annotations {
    /// Fields inherited from a parent profile are prepended, so a child's own
    /// predicates win, and the parent's remain as fallbacks.
    fn inherit(&mut self, parent: &Annotations) {
        fn merge(child: &mut Vec<String>, parent: &[String]) {
            for p in parent {
                if !child.contains(p) {
                    child.push(p.clone());
                }
            }
        }
        merge(&mut self.label, &parent.label);
        merge(&mut self.document_title, &parent.document_title);
        merge(&mut self.document_description, &parent.document_description);
        merge(&mut self.definition, &parent.definition);
        merge(&mut self.comment, &parent.comment);
        merge(&mut self.alt_label, &parent.alt_label);
        merge(&mut self.note, &parent.note);
        merge(&mut self.example, &parent.example);
        merge(&mut self.see_also, &parent.see_also);
        merge(&mut self.status, &parent.status);
        merge(&mut self.deprecated, &parent.deprecated);
        merge(&mut self.replaced_by, &parent.replaced_by);
        merge(&mut self.created, &parent.created);
        merge(&mut self.modified, &parent.modified);
        merge(&mut self.issued, &parent.issued);
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub detect: Detect,
    #[serde(default)]
    pub annotations: Annotations,
}

/// What a document and its file look like, for profile detection.
pub struct Signals<'a> {
    pub document_types: &'a [String],
    pub file_types: &'a [String],
    pub file_predicates: &'a [String],
}

impl Profile {
    fn matches(&self, s: &Signals<'_>) -> bool {
        let any = |wanted: &[String], present: &[String]| {
            wanted.is_empty() || wanted.iter().any(|w| present.contains(w))
        };
        any(&self.detect.document_type, s.document_types)
            && any(&self.detect.file_has_type, s.file_types)
            && any(&self.detect.file_has_predicate, s.file_predicates)
    }
}

/// Every profile available to a run, resolved so that `extends` is applied.
#[derive(Debug, Clone)]
pub struct Registry {
    profiles: BTreeMap<String, Profile>,
}

impl Registry {
    /// Load the built-in profiles and resolve inheritance.
    pub fn built_in() -> Result<Self> {
        let mut raw: BTreeMap<String, Profile> = BTreeMap::new();
        for (name, text) in BUILT_IN {
            let p: Profile =
                toml::from_str(text).with_context(|| format!("parsing built-in profile {name}"))?;
            raw.insert(p.id.clone(), p);
        }
        let resolved = resolve(raw)?;
        Ok(Self { profiles: resolved })
    }

    /// Add or replace a profile from a TOML file on disk.
    pub fn load_file(&mut self, path: &camino::Utf8Path) -> Result<()> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let p: Profile = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
        let mut raw: BTreeMap<String, Profile> = self.profiles.clone();
        raw.insert(p.id.clone(), p);
        self.profiles = resolve(raw)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.get(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.profiles.keys().map(String::as_str).collect()
    }

    /// The highest-priority profile whose detection rules all match.
    ///
    /// Ties are broken by profile id so that detection is deterministic.
    pub fn detect(&self, signals: &Signals<'_>) -> &Profile {
        let mut best: Option<&Profile> = None;
        for p in self.profiles.values() {
            if !p.matches(signals) {
                continue;
            }
            best = match best {
                Some(b)
                    if (b.detect.priority, b.id.as_str()) >= (p.detect.priority, p.id.as_str()) =>
                {
                    Some(b)
                }
                _ => Some(p),
            };
        }
        best.or_else(|| self.profiles.get("generic"))
            .expect("the generic profile is always present")
    }
}

fn resolve(raw: BTreeMap<String, Profile>) -> Result<BTreeMap<String, Profile>> {
    let mut out = BTreeMap::new();
    for (id, profile) in &raw {
        let mut p = profile.clone();
        let mut seen = vec![p.id.clone()];
        let mut parent_id = p.extends.clone();
        while let Some(pid) = parent_id {
            if seen.contains(&pid) {
                anyhow::bail!("profile inheritance cycle at {pid}");
            }
            let parent = raw
                .get(&pid)
                .with_context(|| format!("profile {} extends unknown profile {pid}", p.id))?;
            p.annotations.inherit(&parent.annotations);
            seen.push(pid);
            parent_id = parent.extends.clone();
        }
        out.insert(id.clone(), p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab;

    #[test]
    fn built_in_profiles_parse_and_inherit() {
        let r = Registry::built_in().expect("built-in profiles parse");
        assert_eq!(r.ids(), vec!["dcap", "dcmi", "generic", "shacl", "skos"]);
        let dcap = r.get("dcap").unwrap();
        // Its own predicates come first, the generic fallbacks after.
        assert_eq!(dcap.annotations.definition[0], vocab::RDFS_COMMENT);
        assert!(
            dcap.annotations
                .definition
                .contains(&vocab::DCTERMS_DESCRIPTION.to_owned())
        );
    }

    #[test]
    fn shapes_document_beats_ontology_and_scheme() {
        let r = Registry::built_in().unwrap();
        let doc_types = vec![vocab::OWL_ONTOLOGY.to_owned()];
        let file_types = vec![vocab::SH_NODE_SHAPE.to_owned()];
        let p = r.detect(&Signals {
            document_types: &doc_types,
            file_types: &file_types,
            file_predicates: &[],
        });
        assert_eq!(p.id, "shacl");
    }

    #[test]
    fn concept_scheme_beats_ontology() {
        let r = Registry::built_in().unwrap();
        let doc_types = vec![
            vocab::SKOS_CONCEPT_SCHEME.to_owned(),
            vocab::OWL_ONTOLOGY.to_owned(),
        ];
        let file_types = vec![vocab::SKOS_CONCEPT.to_owned()];
        let p = r.detect(&Signals {
            document_types: &doc_types,
            file_types: &file_types,
            file_predicates: &[],
        });
        assert_eq!(p.id, "skos");
    }

    #[test]
    fn owl_ontology_with_classes_is_dcap() {
        let r = Registry::built_in().unwrap();
        let doc_types = vec![vocab::OWL_ONTOLOGY.to_owned()];
        let file_types = vec![vocab::OWL_CLASS.to_owned()];
        let p = r.detect(&Signals {
            document_types: &doc_types,
            file_types: &file_types,
            file_predicates: &[],
        });
        assert_eq!(p.id, "dcap");
    }
}
