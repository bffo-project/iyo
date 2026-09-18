//! Version segments and the snapshots they name.
//!
//! A vocabulary that keeps only its newest state cannot honour a citation.
//! `docs/output-convention.md` asks for a complete copy of a namespace
//! under a version segment, so that `owl:versionIRI` resolves to the release
//! it names rather than to whatever the vocabulary has become.
//!
//! Two rules keep a segment from breaking the namespace it lives in. It must
//! not equal any local name, or a term and a release would claim the same
//! path. And it must look like a version, because a segment read from
//! `owl:versionInfo` is free text: "draft", "see the changelog" and an empty
//! string are all things a vocabulary has said, and none of them should
//! become a directory.

use crate::model::{Document, Release};
use serde::{Deserialize, Serialize};

/// When a namespace gets a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Only where `owl:versionIRI` is declared and points into the namespace,
    /// which is the case the convention makes a MUST. This is the default
    /// because a snapshot doubles a namespace's output, and a version IRI is
    /// the publisher saying that a particular URL must keep working.
    #[default]
    VersionIri,
    /// Every namespace that carries any version string.
    All,
    None,
}

impl Policy {
    pub fn parse(value: &str) -> Option<Policy> {
        match value {
            "version-iri" => Some(Policy::VersionIri),
            "all" => Some(Policy::All),
            "none" => Some(Policy::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Policy::VersionIri => "version-iri",
            Policy::All => "all",
            Policy::None => "none",
        }
    }
}

/// Where a version string came from, which the page and `versions.json` say
/// out loud: a reader should not have to guess whether `2026-04-28` is a
/// release number or the day the file was last touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    VersionInfo,
    Issued,
    Modified,
    Configured,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::VersionInfo => "owl:versionInfo",
            Source::Issued => "dcterms:issued",
            Source::Modified => "dcterms:modified",
            Source::Configured => "the release option",
        }
    }
}

/// A namespace's snapshot: the segment, where the version came from, and
/// whether the declared version IRI actually lands on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub namespace: String,
    pub segment: String,
    pub source: Source,
    /// The URL the snapshot's document is published at.
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_iri: Option<String>,
    /// True when `owl:versionIRI` equals the snapshot URL, which is what
    /// makes the version IRI resolve (FOOPS! VER2).
    pub version_iri_resolves: bool,
}

/// Whether a string is safe and meaningful as a path segment for a release.
///
/// Semver-ish (`1`, `1.2`, `0.1.0-draft`) or a date (`2026-04-28`). Anything
/// else is prose, and a namespace should not grow a directory called
/// "under development".
pub fn looks_like_a_version(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    // No path or escaping trouble, and nothing that reads as a relative path.
    if value
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '+'))
    {
        return false;
    }
    if value.starts_with('.') || value.starts_with('-') {
        return false;
    }
    let core = value
        .split_once(['-', '+'])
        .map(|(head, _)| head)
        .unwrap_or(value);
    if core.is_empty() {
        return false;
    }
    // A dotted number, or a date, or a bare number: every part is digits.
    let numeric = core
        .split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let dated = core
        .split('-')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    numeric || (dated && core.contains('-'))
}

/// The version string of one document, and where it was read from.
pub fn version_of(doc: &Document, configured: Option<&str>) -> Option<(String, Source)> {
    let candidates = [
        (doc.header.version_info.as_deref(), Source::VersionInfo),
        (doc.header.issued.as_deref(), Source::Issued),
        (doc.header.modified.as_deref(), Source::Modified),
        (configured, Source::Configured),
    ];
    for (value, source) in candidates {
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            continue;
        };
        if looks_like_a_version(value) {
            return Some((value.to_owned(), source));
        }
    }
    None
}

/// Plan a snapshot for every namespace the policy covers.
///
/// A namespace is skipped, never renamed, when its version would collide with
/// one of its own term names: losing the snapshot is recoverable, and quietly
/// serving a release where a term should be is not.
pub fn plan(
    release: &Release,
    plan: &crate::site::Plan,
    policy: Policy,
    configured: Option<&str>,
) -> (Vec<Snapshot>, Vec<String>) {
    let mut out = Vec::new();
    let mut refused = Vec::new();
    if policy == Policy::None {
        return (out, refused);
    }

    for ns in &plan.namespaces {
        let Some(doc) = ns.document.as_deref().and_then(|iri| release.document(iri)) else {
            continue;
        };
        let Some((segment, source)) = version_of(doc, configured) else {
            continue;
        };
        let url = format!("{}{}{}/", plan.base_url, ns.mount, segment);
        let version_iri = doc.header.version_iri.clone();
        let resolves = version_iri.as_deref() == Some(url.as_str());

        if policy == Policy::VersionIri && !resolves {
            // Correct, and it used to be silent. Building the same inputs
            // for a different origin dropped every snapshot -- 107 files to
            // 75 on `testdata/mini` -- with nothing in the summary, in
            // `--json` or in `versions.json` to say a release had gone
            // missing. The refusal is right; only the silence was the bug.
            // A namespace with no version IRI at all is the ordinary case
            // under this policy and says nothing: `version-iri` means
            // "write a snapshot where a version IRI points at one". A
            // namespace that declares one which does not match is the
            // surprising case, and the one that was silent.
            if let Some(iri) = version_iri.as_deref() {
                refused.push(format!(
                    "<{}> declares owl:versionIRI <{iri}>, which is not {url}, \
                     so no snapshot was written there; pass --snapshots all to \
                     write one anyway",
                    ns.iri
                ));
            }
            continue;
        }
        if release
            .local_terms()
            .any(|t| t.namespace == ns.iri && t.local_name == segment)
        {
            refused.push(format!(
                "<{}> has a term named {segment:?}, so no snapshot was written there",
                ns.iri
            ));
            continue;
        }

        out.push(Snapshot {
            namespace: ns.iri.clone(),
            segment,
            source,
            url,
            version_iri,
            version_iri_resolves: resolves,
        });
    }
    (out, refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_segment_has_to_look_like_a_version() {
        for good in [
            "1",
            "1.2",
            "0.1.0",
            "0.1.0-draft",
            "2026-04-28",
            "2026-04-28-2",
        ] {
            assert!(looks_like_a_version(good), "{good} should be accepted");
        }
        // Free text from `owl:versionInfo` is common and must not become a
        // directory.
        for bad in [
            "",
            "draft",
            "under development",
            "see the changelog",
            "../escape",
            "/absolute",
            ".hidden",
            "-leading",
            "v1.0",
        ] {
            assert!(!looks_like_a_version(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn the_policy_names_round_trip() {
        for p in [Policy::VersionIri, Policy::All, Policy::None] {
            assert_eq!(Policy::parse(p.as_str()), Some(p));
        }
        assert_eq!(Policy::parse("sometimes"), None);
    }
}
