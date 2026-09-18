//! Stage 5: lint the release.
//!
//! Rule ids are stable and are the interface for `--select` and `--ignore`
//! (`docs/rules.md`, "How `--select` and `--ignore` match"). The acceptance
//! test for this module is that a run over the BFFO files reproduces the
//! findings the lint was designed against.

use crate::load::Store;
use crate::model::{DocumentKind, Release};
use crate::vocab as v;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Every rule id `check` can emit.
///
/// Ids are the interface for `--select` and `--ignore`, so this list is what
/// makes a misspelled filter an error instead of a run that quietly checks
/// nothing. `every_emitted_rule_is_listed` keeps it honest: a rule added to
/// the code and not to this list fails the suite.
pub const RULES: &[&str] = &[
    "header.missing",
    "release.empty-document",
    "release.has-part-missing",
    "release.no-document",
    "release.part-no-is-part-of",
    "release.part-no-status",
    "release.part-no-version-iri",
    "release.prefix-undeclared",
    "release.prefix-unused",
    "site.case-collision",
    "site.reserved-path-collision",
    "term.deprecated-without-replacement",
    "term.duplicate-label",
    "term.no-definition",
    "term.no-is-defined-by",
    "term.no-label",
    "term.untagged-literal",
    "text.count-mismatch",
    "text.mentions-absent-vocab",
];

/// The filter values that match no known rule.
///
/// Matching is by prefix, the same way `--select` and `--ignore` match, so
/// `term.` is a valid filter and `term.no-labell` is not.
pub fn unknown_filters(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|v| !RULES.iter().any(|r| r.starts_with(v.as_str())))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Only report rules whose id starts with one of these.
    pub select: Vec<String>,
    /// Suppress rules whose id starts with one of these.
    pub ignore: Vec<String>,
    /// Report untagged literals, which most vocabularies have by design.
    pub strict_lang: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: &'static str,
    pub findings: Vec<Finding>,
    pub summary: Summary,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    /// Findings `--ignore` removed before they were counted.
    ///
    /// `--ignore` can suppress error-level rules, which turns an exit 1 into
    /// an exit 0. That is its job, but it should never happen without saying
    /// so: a CI configuration that quietly stopped failing is worth noticing.
    pub suppressed: usize,
    pub by_rule: BTreeMap<String, usize>,
}

struct Sink<'a> {
    findings: Vec<Finding>,
    suppressed: usize,
    options: &'a Options,
}

impl Sink<'_> {
    fn add(
        &mut self,
        rule: &str,
        severity: Severity,
        message: impl Into<String>,
        subject: Option<&str>,
        file: Option<&str>,
    ) {
        if !self.options.select.is_empty()
            && !self
                .options
                .select
                .iter()
                .any(|s| rule.starts_with(s.as_str()))
        {
            return;
        }
        if self
            .options
            .ignore
            .iter()
            .any(|s| rule.starts_with(s.as_str()))
        {
            self.suppressed += 1;
            return;
        }
        self.findings.push(Finding {
            rule: rule.to_owned(),
            severity,
            message: message.into(),
            subject: subject.map(str::to_owned),
            file: file.map(str::to_owned),
        });
    }
}

/// Formats whose parsers carry prefix declarations, for the prefix checks.
fn has_prefixes(format: &str) -> bool {
    matches!(format, "turtle" | "trig" | "n3" | "rdfxml" | "jsonld")
}

pub fn run(release: &Release, store: &Store, options: &Options) -> Report {
    let mut sink = Sink {
        findings: Vec::new(),
        suppressed: 0,
        options,
    };

    prefix_rules(release, store, &mut sink);
    release_rules(release, &mut sink);
    header_rules(release, &mut sink);
    term_rules(release, &mut sink);
    site_rules(release, &mut sink);

    sink.findings
        .sort_by(|a, b| (b.severity, &a.rule, &a.subject).cmp(&(a.severity, &b.rule, &b.subject)));

    let mut summary = Summary::default();
    for f in &sink.findings {
        match f.severity {
            Severity::Error => summary.errors += 1,
            Severity::Warning => summary.warnings += 1,
            Severity::Info => summary.info += 1,
        }
        *summary.by_rule.entry(f.rule.clone()).or_default() += 1;
    }
    summary.suppressed = sink.suppressed;

    Report {
        schema_version: crate::model::SCHEMA_VERSION,
        findings: sink.findings,
        summary,
    }
}

/// Declared but never used, and used in a file that does not declare it.
fn prefix_rules(release: &Release, store: &Store, sink: &mut Sink<'_>) {
    let used = store.used_namespaces();

    for file in &release.files {
        for (prefix, namespace) in &file.prefixes {
            if !used.contains(namespace) {
                sink.add(
                    "release.prefix-unused",
                    Severity::Warning,
                    format!(
                        "prefix {prefix}: is declared but its namespace <{namespace}> is never used"
                    ),
                    Some(namespace),
                    Some(&file.path),
                );
            }
        }
    }

    // A namespace that the release names with a prefix somewhere, used in a
    // file that does not declare it.
    let declared_anywhere: BTreeMap<&String, &String> =
        release.prefixes.iter().map(|(p, ns)| (ns, p)).collect();
    for (index, file) in release.files.iter().enumerate() {
        if !has_prefixes(&file.format) {
            continue;
        }
        let mut used_here: BTreeSet<String> = BTreeSet::new();
        for t in store.triples.iter().filter(|t| t.file == index) {
            for iri in [
                match &t.subject {
                    oxrdf::NamedOrBlankNode::NamedNode(n) => Some(n.as_str()),
                    _ => None,
                },
                Some(t.predicate.as_str()),
                crate::load::term_iri(&t.object),
            ]
            .into_iter()
            .flatten()
            {
                if let Some((ns, _)) = v::split_iri(iri) {
                    used_here.insert(ns.to_owned());
                }
            }
        }
        for ns in &used_here {
            let Some(prefix) = declared_anywhere.get(ns) else {
                continue;
            };
            if !file.prefixes.contains_key(prefix.as_str()) {
                sink.add(
                    "release.prefix-undeclared",
                    Severity::Warning,
                    format!(
                        "namespace <{ns}> is used here but the prefix {prefix}: is declared only in other files"
                    ),
                    Some(ns),
                    Some(&file.path),
                );
            }
        }
    }
}

/// Release-level structure: parts, versions, status, the `hasPart` manifest.
fn release_rules(release: &Release, sink: &mut Sink<'_>) {
    let Some(root) = release.root_document() else {
        sink.add(
            "release.no-document",
            Severity::Error,
            "no owl:Ontology or skos:ConceptScheme was found in the inputs",
            None,
            None,
        );
        return;
    };

    let declared_parts: BTreeSet<&String> = root.header.has_part.iter().collect();
    for doc in &release.documents {
        if doc.iri == root.iri {
            continue;
        }
        let file = release.files.get(doc.source_file).map(|f| f.path.as_str());
        if !declared_parts.contains(&doc.iri) {
            sink.add(
                "release.has-part-missing",
                Severity::Warning,
                format!(
                    "document is part of the release but <{}> does not list it in dcterms:hasPart",
                    root.iri
                ),
                Some(&doc.iri),
                file,
            );
        }
        if doc.header.is_part_of.is_empty() {
            sink.add(
                "release.part-no-is-part-of",
                Severity::Warning,
                "document has no dcterms:isPartOf back to the release root",
                Some(&doc.iri),
                file,
            );
        }
        if doc.header.version_iri.is_none() {
            sink.add(
                "release.part-no-version-iri",
                Severity::Warning,
                "document has no owl:versionIRI; the release version is inherited from the root",
                Some(&doc.iri),
                file,
            );
        }
        if doc.header.status.is_none() {
            sink.add(
                "release.part-no-status",
                Severity::Warning,
                "document has no adms:status; the release status is inherited from the root",
                Some(&doc.iri),
                file,
            );
        }
    }
}

/// Header metadata expected by the FOOPS! checks OM1 to OM5.
fn header_rules(release: &Release, sink: &mut Sink<'_>) {
    let Some(root) = release.root_document() else {
        return;
    };
    let file = release.files.get(root.source_file).map(|f| f.path.as_str());
    let h = &root.header;
    let mut missing: Vec<&str> = Vec::new();
    if h.title.is_empty() {
        missing.push("dcterms:title");
    }
    if h.description.is_empty() {
        missing.push("dcterms:description");
    }
    if h.license.is_none() {
        missing.push("dcterms:license");
    }
    if h.creators.is_empty() {
        missing.push("dcterms:creator");
    }
    if h.version_iri.is_none() {
        missing.push("owl:versionIRI");
    }
    if h.version_info.is_none() {
        missing.push("owl:versionInfo");
    }
    if h.prefix.is_none() {
        missing.push("vann:preferredNamespacePrefix");
    }
    if h.namespace_uri.is_none() {
        missing.push("vann:preferredNamespaceUri");
    }
    if h.created.is_none() {
        missing.push("dcterms:created");
    }
    if h.modified.is_none() {
        missing.push("dcterms:modified");
    }
    if h.issued.is_none() {
        missing.push("dcterms:issued");
    }
    if h.citation.is_none() {
        missing.push("dcterms:bibliographicCitation");
    }
    if h.identifier.is_none() {
        missing.push("dcterms:identifier");
    }
    if h.prior_version.is_none() && h.version_info.is_some() {
        missing.push("owl:priorVersion");
    }
    if h.contributors.is_empty() {
        missing.push("dcterms:contributor");
    }
    for property in missing {
        let severity = match property {
            "dcterms:title" | "dcterms:description" | "dcterms:license" => Severity::Warning,
            _ => Severity::Info,
        };
        sink.add(
            "header.missing",
            severity,
            format!("release root has no {property}"),
            Some(&root.iri),
            file,
        );
    }
}

/// Per-term rules.
fn term_rules(release: &Release, sink: &mut Sink<'_>) {
    let mut labels_seen: BTreeMap<String, Vec<&str>> = BTreeMap::new();

    for term in release.terms.iter().filter(|t| !t.foreign) {
        let file = release.files.get(term.source_file).map(|f| f.path.as_str());
        if term.labels.is_empty() {
            sink.add(
                "term.no-label",
                Severity::Error,
                "term has no label",
                Some(&term.iri),
                file,
            );
        }
        if term.definitions.is_empty() {
            sink.add(
                "term.no-definition",
                Severity::Error,
                "term has no definition",
                Some(&term.iri),
                file,
            );
        }
        if term.is_defined_by.is_none() {
            sink.add(
                "term.no-is-defined-by",
                Severity::Warning,
                "term has no rdfs:isDefinedBy pointing at its vocabulary",
                Some(&term.iri),
                file,
            );
        }
        if term.deprecated && term.replaced_by.is_empty() {
            sink.add(
                "term.deprecated-without-replacement",
                Severity::Warning,
                "term is deprecated but has no dcterms:isReplacedBy",
                Some(&term.iri),
                file,
            );
        }
        for label in &term.labels {
            labels_seen
                .entry(label.value.to_lowercase())
                .or_default()
                .push(&term.iri);
        }
        if sink.options.strict_lang {
            for label in term.labels.iter().chain(term.definitions.iter()) {
                if label.lang.is_none() {
                    sink.add(
                        "term.untagged-literal",
                        Severity::Info,
                        format!("literal from <{}> has no language tag", label.source),
                        Some(&term.iri),
                        file,
                    );
                    break;
                }
            }
        }
    }

    for (value, iris) in labels_seen {
        if iris.len() > 1 {
            sink.add(
                "term.duplicate-label",
                Severity::Warning,
                format!(
                    "label {value:?} is used by {} terms: {}",
                    iris.len(),
                    iris.join(", ")
                ),
                Some(iris[0]),
                None,
            );
        }
    }
}

/// Rules about the site the release will become.
fn site_rules(release: &Release, sink: &mut Sink<'_>) {
    for ns in &release.namespaces {
        for term in release
            .terms
            .iter()
            .filter(|t| !t.foreign && t.namespace == ns.iri)
        {
            if ns.reserved.contains(&term.local_name) {
                sink.add(
                    "site.reserved-path-collision",
                    Severity::Error,
                    format!(
                        "local name {:?} collides with a reserved path segment of <{}>",
                        term.local_name, ns.iri
                    ),
                    Some(&term.iri),
                    release.files.get(term.source_file).map(|f| f.path.as_str()),
                );
            }
        }
    }

    // Names that fold together on a case-insensitive filesystem. The build
    // handles this by giving all but the first the directory layout, so
    // nothing is lost, but the affected term is then served from a URL with a
    // trailing slash and the maintainer should know which.
    for ns in &release.namespaces {
        let mut folded: BTreeMap<String, Vec<&crate::model::Term>> = BTreeMap::new();
        for term in release
            .terms
            .iter()
            .filter(|t| !t.foreign && t.namespace == ns.iri)
        {
            folded
                .entry(term.local_name.to_lowercase())
                .or_default()
                .push(term);
        }
        for (_, mut group) in folded {
            if group.len() < 2 {
                continue;
            }
            group.sort_by(|a, b| a.local_name.cmp(&b.local_name));
            let names: Vec<&str> = group.iter().map(|t| t.local_name.as_str()).collect();
            for term in group.iter().skip(1) {
                sink.add(
                    "site.case-collision",
                    Severity::Warning,
                    format!(
                        "local names {names:?} differ only by case, so they are one file on \
                         a case-insensitive filesystem; {:?} is published at a directory URL \
                         instead so that both survive",
                        term.local_name
                    ),
                    Some(&term.iri),
                    release.files.get(term.source_file).map(|f| f.path.as_str()),
                );
            }
        }
    }

    for doc in &release.documents {
        if doc.kind == DocumentKind::Ontology
            && doc.terms.is_empty()
            && doc.foreign_terms.is_empty()
        {
            sink.add(
                "release.empty-document",
                Severity::Info,
                "document declares no terms",
                Some(&doc.iri),
                release.files.get(doc.source_file).map(|f| f.path.as_str()),
            );
        }
    }

    // The abstract is prose that goes stale; surface a count mismatch the way
    // A real release said "centered on three classes" against four declared.
    if let Some(root) = release.root_document() {
        let classes = release
            .terms
            .iter()
            .filter(|t| !t.foreign && t.kind == crate::model::TermKind::Class)
            .count();
        let words = [
            ("one", 1usize),
            ("two", 2),
            ("three", 3),
            ("four", 4),
            ("five", 5),
            ("six", 6),
            ("seven", 7),
            ("eight", 8),
            ("nine", 9),
            ("ten", 10),
        ];
        for text in root
            .header
            .abstract_
            .iter()
            .chain(root.header.description.iter())
        {
            let lower = text.value.to_lowercase();
            for (word, n) in words {
                if lower.contains(&format!("{word} classes")) && n != classes {
                    sink.add(
                        "text.count-mismatch",
                        Severity::Warning,
                        format!("prose says \"{word} classes\" but the release declares {classes}"),
                        Some(&root.iri),
                        release.files.get(root.source_file).map(|f| f.path.as_str()),
                    );
                }
            }
        }
    }

    // Prose that names a vocabulary the graph does not use.
    if let Some(root) = release.root_document() {
        let used: BTreeSet<&String> = release.prefixes.values().collect();
        let watch = [
            ("DOAP", "http://usefulinc.com/ns/doap#"),
            ("ADMS", v::ADMS),
            ("DCAT", v::DCAT),
            ("PROV", v::PROV),
        ];
        for text in root
            .header
            .abstract_
            .iter()
            .chain(root.header.description.iter())
        {
            for (name, ns) in watch {
                if text.value.contains(name) && !used.iter().any(|u| u.as_str() == ns) {
                    sink.add(
                        "text.mentions-absent-vocab",
                        Severity::Warning,
                        format!("prose names {name} but the release declares no {ns} prefix"),
                        Some(&root.iri),
                        release.files.get(root.source_file).map(|f| f.path.as_str()),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod rule_inventory {
    use super::RULES;

    /// Every rule id the code can emit is in `RULES`.
    ///
    /// `RULES` is what makes a misspelled `--select` an error, and it is the
    /// list the reference documentation publishes. A rule added to the code
    /// and not to the list would silently drop out of both: the filter would
    /// reject a legitimate id, and the docs would omit a rule. Scanning this
    /// file is crude, and it is the only thing that ties the three together.
    #[test]
    fn every_emitted_rule_is_listed() {
        let whole = include_str!("check.rs");
        // Stop before the test modules, or the scan finds the string literals
        // in its own body and reports them as rule ids.
        let source = whole.split("#[cfg(test)]").next().unwrap_or(whole);
        let mut emitted: Vec<String> = Vec::new();
        for (i, _) in source
            .match_indices("sink.add(")
            .chain(source.match_indices("self.add("))
        {
            // The rule id is the first string literal after the call.
            let rest = &source[i..];
            if let Some(start) = rest.find('"')
                && let Some(len) = rest[start + 1..].find('"')
            {
                let id = &rest[start + 1..start + 1 + len];
                if id.contains('.') && !id.contains(' ') {
                    emitted.push(id.to_owned());
                }
            }
        }
        emitted.sort();
        emitted.dedup();
        assert!(
            !emitted.is_empty(),
            "the scan found no rule ids at all, so it is not checking anything"
        );
        let missing: Vec<&String> = emitted
            .iter()
            .filter(|e| !RULES.contains(&e.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "these rule ids are emitted but absent from RULES, so --select would \
             reject them and the docs would omit them: {missing:?}"
        );
    }

    /// And nothing in `RULES` is fictional.
    #[test]
    fn every_listed_rule_is_documented() {
        let docs = include_str!("../docs/rules.md");
        let undocumented: Vec<&&str> = RULES
            .iter()
            .filter(|r| !docs.contains(&format!("`{r}`")))
            .collect();
        assert!(
            undocumented.is_empty(),
            "rule ids are a published interface; these are in RULES but not in \
             docs/rules.md: {undocumented:?}"
        );
    }
}
