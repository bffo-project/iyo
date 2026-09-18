//! What changed between two releases.
//!
//! A vocabulary's users need to know three different things and a single list
//! of edits tells them none of them: what will break, what they can start
//! using, and what merely reads differently. So every change carries a
//! severity, and the changelog is grouped by it rather than by the order the
//! comparison happened to find things.
//!
//! Severity is about consumers, not about effort. Removing a term breaks
//! anyone who used it; adding one breaks nobody; rewording a definition
//! breaks nobody but is worth reading. Narrowing a constraint is breaking
//! because data that validated may stop validating, and widening one is not,
//! which is a distinction a diff of the RDF alone cannot make and a diff that
//! understands SHACL can.

use crate::model::{Release, Term};
use crate::shape::PropertyShape;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A consumer that worked against the old release may now fail.
    Breaking,
    /// Something new is available; nothing that worked stops working.
    Additive,
    /// The same meaning, said differently.
    Editorial,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Breaking => "Breaking",
            Severity::Additive => "Additive",
            Severity::Editorial => "Editorial",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// Stable id, for example `term.removed`.
    pub rule: &'static str,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// The anchor the term had on its document page, so that a link into the
    /// old release can be answered rather than 404'd.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Side {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub terms: usize,
    pub documents: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    /// The version of this document's shape, first field so a consumer can
    /// branch on it before anything else (`docs/cli.md`, "`--json`": it is
    /// the stable interface).
    pub schema_version: &'static str,
    /// The namespace this changelog covers, when it covers only one. A
    /// scoped changelog must not quote release-wide totals beside a list of
    /// one namespace's changes: the two numbers would not be about the same
    /// thing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub old: Side,
    pub new: Side,
    pub counts: BTreeMap<String, usize>,
    pub breaking: usize,
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

fn side(release: &Release) -> Side {
    Side {
        version: release
            .root_document()
            .and_then(|d| d.header.version_info.clone()),
        terms: release.local_terms().count(),
        documents: release.documents.len(),
    }
}

fn label_of(term: &Term) -> String {
    term.display(COMPARE_LANGUAGE).to_owned()
}

/// The language to compare labels and definitions in.
///
/// One language, not all of them: comparing every tag would report a
/// relabelling when a translation is added, which is additive. The default
/// matches the site default, since a release carries no language of its own.
const COMPARE_LANGUAGE: &str = "en";

fn definition_of(term: &Term) -> Option<String> {
    term.definition(COMPARE_LANGUAGE).map(|d| d.value.clone())
}

fn set(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

fn joined(values: &BTreeSet<&str>) -> String {
    values.iter().copied().collect::<Vec<_>>().join(", ")
}

/// Compare two releases.
pub fn run(old: &Release, new: &Release) -> Diff {
    let mut changes: Vec<Change> = Vec::new();

    let old_terms: BTreeMap<&str, &Term> = old.local_terms().map(|t| (t.iri.as_str(), t)).collect();
    let new_terms: BTreeMap<&str, &Term> = new.local_terms().map(|t| (t.iri.as_str(), t)).collect();

    for (iri, term) in &old_terms {
        if new_terms.contains_key(iri) {
            continue;
        }
        // A term that is gone is the one change that certainly breaks
        // someone, and the one a changelog most often forgets to list.
        changes.push(Change {
            rule: "term.removed",
            severity: Severity::Breaking,
            iri: Some((*iri).to_owned()),
            label: Some(label_of(term)),
            namespace: Some(term.namespace.clone()),
            anchor: Some(term.anchor.clone()),
            detail: match definition_of(term) {
                Some(d) => format!("was {:?}", d),
                None => "no definition was recorded".to_owned(),
            },
        });
    }

    for (iri, term) in &new_terms {
        if old_terms.contains_key(iri) {
            continue;
        }
        changes.push(Change {
            rule: "term.added",
            severity: Severity::Additive,
            iri: Some((*iri).to_owned()),
            label: Some(label_of(term)),
            namespace: Some(term.namespace.clone()),
            anchor: Some(term.anchor.clone()),
            detail: definition_of(term).unwrap_or_else(|| "no definition".to_owned()),
        });
    }

    for (iri, before) in &old_terms {
        let Some(after) = new_terms.get(iri) else {
            continue;
        };
        compare_term(before, after, &mut changes);
    }

    compare_constraints(old, new, &mut changes);
    compare_documents(old, new, &mut changes);

    changes.sort_by(|a, b| {
        (a.severity, a.rule, &a.iri, &a.detail).cmp(&(b.severity, b.rule, &b.iri, &b.detail))
    });

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for c in &changes {
        *counts.entry(c.rule.to_owned()).or_default() += 1;
    }
    Diff {
        schema_version: crate::model::SCHEMA_VERSION,
        scope: None,
        old: side(old),
        new: side(new),
        breaking: changes
            .iter()
            .filter(|c| c.severity == Severity::Breaking)
            .count(),
        counts,
        changes,
    }
}

fn push(
    changes: &mut Vec<Change>,
    rule: &'static str,
    severity: Severity,
    term: &Term,
    detail: String,
) {
    changes.push(Change {
        rule,
        severity,
        iri: Some(term.iri.clone()),
        label: Some(label_of(term)),
        namespace: Some(term.namespace.clone()),
        anchor: Some(term.anchor.clone()),
        detail,
    });
}

fn compare_term(before: &Term, after: &Term, changes: &mut Vec<Change>) {
    if before.kind != after.kind {
        // A class that becomes a property invalidates every use of it.
        push(
            changes,
            "term.kind-changed",
            Severity::Breaking,
            after,
            format!(
                "was a {}, now a {}",
                before.kind.label(),
                after.kind.label()
            ),
        );
    }

    if !before.deprecated && after.deprecated {
        let replacement = if after.replaced_by.is_empty() {
            "no replacement is recorded".to_owned()
        } else {
            format!("use {}", after.replaced_by.join(", "))
        };
        // Deprecation is a warning, not a removal: the term still resolves,
        // so nothing breaks yet.
        push(
            changes,
            "term.deprecated",
            Severity::Additive,
            after,
            replacement,
        );
    }
    if before.deprecated && !after.deprecated {
        push(
            changes,
            "term.undeprecated",
            Severity::Additive,
            after,
            "the deprecation was withdrawn".to_owned(),
        );
    }

    let (old_label, new_label) = (label_of(before), label_of(after));
    if old_label != new_label {
        push(
            changes,
            "term.relabelled",
            Severity::Editorial,
            after,
            format!("{old_label:?} became {new_label:?}"),
        );
    }

    match (definition_of(before), definition_of(after)) {
        (Some(a), Some(b)) if a != b => push(
            changes,
            "term.redefined",
            Severity::Editorial,
            after,
            format!("was {a:?}"),
        ),
        (Some(_), None) => push(
            changes,
            "term.definition-removed",
            Severity::Editorial,
            after,
            "the definition was removed".to_owned(),
        ),
        (None, Some(_)) => push(
            changes,
            "term.defined",
            Severity::Additive,
            after,
            "a definition was added".to_owned(),
        ),
        _ => {}
    }

    let (a, b) = (set(&before.super_terms), set(&after.super_terms));
    if a != b {
        let lost: BTreeSet<&str> = a.difference(&b).copied().collect();
        let gained: BTreeSet<&str> = b.difference(&a).copied().collect();
        // Losing a parent takes entailments away from data that relied on
        // them; gaining one only adds.
        if !lost.is_empty() {
            push(
                changes,
                "term.parent-removed",
                Severity::Breaking,
                after,
                format!("no longer under {}", joined(&lost)),
            );
        }
        if !gained.is_empty() {
            push(
                changes,
                "term.parent-added",
                Severity::Additive,
                after,
                format!("now under {}", joined(&gained)),
            );
        }
    }

    for (name, rule, before_values, after_values) in [
        (
            "domain",
            "term.domain-changed",
            &before.property.domain,
            &after.property.domain,
        ),
        (
            "range",
            "term.range-changed",
            &before.property.range,
            &after.property.range,
        ),
    ] {
        let (a, b) = (set(before_values), set(after_values));
        if a != b {
            push(
                changes,
                rule,
                Severity::Breaking,
                after,
                format!(
                    "{name} was {}, now {}",
                    if a.is_empty() {
                        "unstated".to_owned()
                    } else {
                        joined(&a)
                    },
                    if b.is_empty() {
                        "unstated".to_owned()
                    } else {
                        joined(&b)
                    }
                ),
            );
        }
    }
}

/// What the shapes say about each property, compared.
///
/// This is the part a diff of the triples cannot do. Two releases can have
/// identical vocabularies and completely different rules about what a record
/// must contain, and the direction of a cardinality change decides whether
/// existing data still validates.
fn compare_constraints(old: &Release, new: &Release, changes: &mut Vec<Change>) {
    let properties: BTreeSet<&str> = old
        .local_terms()
        .chain(new.local_terms())
        .map(|t| t.iri.as_str())
        .collect();

    for iri in properties {
        let before = old.shapes.for_property(iri);
        let after = new.shapes.for_property(iri);
        let by_shape = |list: &[(&crate::shape::NodeShape, &PropertyShape)]| {
            list.iter()
                .map(|(s, p)| (s.iri.clone(), (*p).clone()))
                .collect::<BTreeMap<String, PropertyShape>>()
        };
        let (before, after) = (by_shape(&before), by_shape(&after));

        for (shape, was) in &before {
            let Some(now) = after.get(shape) else {
                changes.push(constraint_change(
                    "constraint.removed",
                    Severity::Additive,
                    new,
                    iri,
                    format!("{shape} no longer constrains it"),
                ));
                continue;
            };
            if was.required() != now.required() {
                // Making a field required rejects records that were valid.
                let (rule, severity) = if now.required() {
                    ("constraint.now-required", Severity::Breaking)
                } else {
                    ("constraint.now-optional", Severity::Additive)
                };
                changes.push(constraint_change(
                    rule,
                    severity,
                    new,
                    iri,
                    format!(
                        "{shape}: {} to {}",
                        if was.required() {
                            "required"
                        } else {
                            "optional"
                        },
                        if now.required() {
                            "required"
                        } else {
                            "optional"
                        }
                    ),
                ));
            }
            if was.cardinality() != now.cardinality() && was.required() == now.required() {
                // Narrower in either direction: a lower ceiling rejects
                // records with too many values, a higher floor rejects
                // records with too few. Only the ceiling is obvious, and
                // reading `1..*` to `2..*` as additive is exactly the mistake
                // a changelog should not make.
                let lower_ceiling =
                    now.max_count.unwrap_or(u64::MAX) < was.max_count.unwrap_or(u64::MAX);
                let higher_floor = now.min_count.unwrap_or(0) > was.min_count.unwrap_or(0);
                let narrower = lower_ceiling || higher_floor;
                changes.push(constraint_change(
                    "constraint.cardinality-changed",
                    if narrower {
                        Severity::Breaking
                    } else {
                        Severity::Additive
                    },
                    new,
                    iri,
                    format!(
                        "{shape}: {} to {}",
                        was.cardinality().unwrap_or_else(|| "any".to_owned()),
                        now.cardinality().unwrap_or_else(|| "any".to_owned())
                    ),
                ));
            }
            if was.datatypes != now.datatypes {
                let narrower = now.datatypes.len() < was.datatypes.len();
                changes.push(constraint_change(
                    "constraint.datatype-changed",
                    if narrower {
                        Severity::Breaking
                    } else {
                        Severity::Additive
                    },
                    new,
                    iri,
                    format!(
                        "{shape}: {} to {}",
                        was.datatypes.join(" or "),
                        now.datatypes.join(" or ")
                    ),
                ));
            }
            if was.in_scheme != now.in_scheme {
                changes.push(constraint_change(
                    "constraint.scheme-changed",
                    Severity::Breaking,
                    new,
                    iri,
                    format!(
                        "{shape}: values now come from {}",
                        if now.in_scheme.is_empty() {
                            "anywhere".to_owned()
                        } else {
                            now.in_scheme.join(", ")
                        }
                    ),
                ));
            }
            if was.values != now.values {
                let narrower = now.values.len() < was.values.len();
                changes.push(constraint_change(
                    "constraint.values-changed",
                    if narrower {
                        Severity::Breaking
                    } else {
                        Severity::Additive
                    },
                    new,
                    iri,
                    format!(
                        "{shape}: {} to {}",
                        was.values.join(", "),
                        now.values.join(", ")
                    ),
                ));
            }
        }

        for shape in after.keys() {
            if !before.contains_key(shape) {
                changes.push(constraint_change(
                    "constraint.added",
                    Severity::Breaking,
                    new,
                    iri,
                    format!("{shape} now constrains it"),
                ));
            }
        }
    }
}

fn constraint_change(
    rule: &'static str,
    severity: Severity,
    release: &Release,
    iri: &str,
    detail: String,
) -> Change {
    let term = release.term(iri);
    Change {
        rule,
        severity,
        iri: Some(iri.to_owned()),
        label: term.map(label_of),
        namespace: term.map(|t| t.namespace.clone()),
        anchor: term.map(|t| t.anchor.clone()),
        detail,
    }
}

fn compare_documents(old: &Release, new: &Release, changes: &mut Vec<Change>) {
    let old_docs: BTreeMap<&str, _> = old.documents.iter().map(|d| (d.iri.as_str(), d)).collect();
    let new_docs: BTreeMap<&str, _> = new.documents.iter().map(|d| (d.iri.as_str(), d)).collect();

    for (iri, doc) in &old_docs {
        if !new_docs.contains_key(iri) {
            changes.push(Change {
                rule: "document.removed",
                severity: Severity::Breaking,
                iri: Some((*iri).to_owned()),
                label: Some(doc.display(COMPARE_LANGUAGE).to_owned()),
                namespace: Some((*iri).to_owned()),
                anchor: None,
                detail: "the whole document is gone".to_owned(),
            });
        }
    }
    for (iri, doc) in &new_docs {
        let Some(before) = old_docs.get(iri) else {
            changes.push(Change {
                rule: "document.added",
                severity: Severity::Additive,
                iri: Some((*iri).to_owned()),
                label: Some(doc.display(COMPARE_LANGUAGE).to_owned()),
                namespace: Some((*iri).to_owned()),
                anchor: None,
                detail: "a new document".to_owned(),
            });
            continue;
        };
        if before.header.version_info != doc.header.version_info {
            changes.push(Change {
                rule: "document.version-changed",
                severity: Severity::Editorial,
                iri: Some((*iri).to_owned()),
                label: Some(doc.display(COMPARE_LANGUAGE).to_owned()),
                namespace: Some((*iri).to_owned()),
                anchor: None,
                detail: format!(
                    "{} to {}",
                    before.header.version_info.as_deref().unwrap_or("unstated"),
                    doc.header.version_info.as_deref().unwrap_or("unstated")
                ),
            });
        }
    }
}

/// A rule id as a section heading: `constraint.values-changed` reads as
/// "Constraint values changed", not as an identifier with a full stop in it.
fn heading(rule: &str) -> String {
    let words = rule.replace(['.', '-', '_'], " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => words,
    }
}

/// The changelog, as Markdown.
pub fn markdown(diff: &Diff, title: &str) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let version = diff.new.version.as_deref().unwrap_or("this release");
    let _ = writeln!(s, "# Changes in {title} {version}\n");
    match &diff.old.version {
        Some(old) => {
            let _ = writeln!(s, "Compared with {old}.\n");
        }
        None => {
            let _ = writeln!(s, "Compared with the previous release.\n");
        }
    }

    if diff.is_empty() {
        let _ = writeln!(s, "Nothing changed.\n");
        return s;
    }

    let totals = match diff.scope {
        Some(_) => String::new(),
        None => format!(
            " {} terms before, {} after.",
            diff.old.terms, diff.new.terms
        ),
    };
    let _ = writeln!(
        s,
        "{} change{}, {} of them breaking.{totals}\n",
        diff.changes.len(),
        if diff.changes.len() == 1 { "" } else { "s" },
        diff.breaking
    );

    for severity in [Severity::Breaking, Severity::Additive, Severity::Editorial] {
        let group: Vec<&Change> = diff
            .changes
            .iter()
            .filter(|c| c.severity == severity)
            .collect();
        if group.is_empty() {
            continue;
        }
        let _ = writeln!(s, "## {}\n", severity.label());
        if severity == Severity::Breaking {
            let _ = writeln!(
                s,
                "A consumer that worked against the previous release may now fail.\n"
            );
        }
        let mut by_rule: BTreeMap<&str, Vec<&Change>> = BTreeMap::new();
        for c in group {
            by_rule.entry(c.rule).or_default().push(c);
        }
        for (rule, items) in by_rule {
            let _ = writeln!(s, "### {}\n", heading(rule));
            for c in items {
                // A removed term keeps its old anchor here, so that a link
                // into the previous release lands on the note explaining
                // where it went rather than on a 404.
                if rule == "term.removed"
                    && let Some(anchor) = &c.anchor
                {
                    let _ = writeln!(s, "<a id=\"{anchor}\"></a>");
                }
                let name = c.label.as_deref().unwrap_or("");
                match &c.iri {
                    Some(iri) => {
                        let _ = writeln!(s, "- **{name}** (`{iri}`): {}", c.detail);
                    }
                    None => {
                        let _ = writeln!(s, "- {}", c.detail);
                    }
                }
            }
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_floor_is_as_breaking_as_a_lower_ceiling() {
        let card = |min: Option<u64>, max: Option<u64>| PropertyShape {
            min_count: min,
            max_count: max,
            ..PropertyShape::default()
        };
        // Both of these reject records that used to validate.
        let (was, now) = (card(Some(1), None), card(Some(2), None));
        assert!(now.min_count > was.min_count);
        let (was2, now2) = (card(Some(0), Some(3)), card(Some(0), Some(1)));
        assert!(now2.max_count < was2.max_count);
        // Widening is not breaking.
        let (was3, now3) = (card(Some(1), Some(1)), card(Some(1), None));
        assert!(now3.max_count.unwrap_or(u64::MAX) > was3.max_count.unwrap_or(u64::MAX));
    }

    #[test]
    fn a_rule_id_becomes_a_readable_heading() {
        assert_eq!(
            heading("constraint.values-changed"),
            "Constraint values changed"
        );
        assert_eq!(heading("term.removed"), "Term removed");
    }

    #[test]
    fn severity_orders_breaking_first() {
        let mut all = vec![Severity::Editorial, Severity::Breaking, Severity::Additive];
        all.sort();
        assert_eq!(
            all,
            vec![Severity::Breaking, Severity::Additive, Severity::Editorial]
        );
    }
}
