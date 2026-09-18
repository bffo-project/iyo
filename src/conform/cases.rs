//! The contract, and the roles it is written in terms of.
//!
//! Cases name a subject role rather than a path, so one file checks the
//! fixture and a deployment with the same sentences. `apache-rules.py` had
//! BFFO's paths compiled into it once and reported twelve failures on the
//! fixture its own README told you to build; a checker with a vocabulary
//! baked in is a checker for one vocabulary.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// How a subject is chosen from a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Term,
    DirTerm,
    Sibling,
    Namespace,
    NestedNamespace,
    ReleaseTerm,
    EmptyNamespace,
    /// A reserved segment that is not also a version segment, requested as
    /// its own turtle sibling. Restored after being deleted twice for the
    /// same reason and reinstated for a narrower one: the convention does not
    /// define which of 404/301/200 the file layer answers for the bare segment
    /// (`/vocab/shapes`), but it does define that the segment must not resolve
    /// as a term of its parent -- that is what `reserved` exists for, and what
    /// `apache.rs` and `vercel.rs` implement as exclusion lists. Asking for
    /// the representation-shaped path instead of the bare one tests exactly
    /// the defined half.
    ReservedSegment,
    SubTermPath,
    AbsentName,
    AbsentRelease,
    CaseVariant,
    SiblingUnpublished,
    AbsentReleaseName,
}

impl Role {
    /// Every role, so a test can assert the contract exercises all of them.
    pub fn all() -> [Role; 14] {
        use Role::*;
        [
            Term,
            DirTerm,
            Sibling,
            Namespace,
            NestedNamespace,
            ReleaseTerm,
            EmptyNamespace,
            ReservedSegment,
            SubTermPath,
            AbsentName,
            AbsentRelease,
            CaseVariant,
            SiblingUnpublished,
            AbsentReleaseName,
        ]
    }
}

/// What a request should produce. `pass` from the old matrix splits into
/// `File` and `Absent`: against a real host "a file is there" and "nothing is
/// there" are opposite results, and conflating them is what lets an
/// application's catch-all route answer 200 for an IRI nobody minted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Expect {
    /// 200, this media type, and a body naming the subject's identity IRI.
    Serve {
        #[serde(rename = "as")]
        media_type: String,
    },
    /// The manifest's `status_code`, and `Location` equal to the sibling.
    Redirect { media_type: String },
    /// Asked for by name; not a negotiation.
    File { media_type: String },
    /// 404, and explicitly not 200.
    Absent,
}

/// One case, before a manifest resolves it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Case {
    pub name: String,
    pub subject: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub expect: Expect,
    /// What the summary groups this under.
    pub group: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Contract {
    pub cases: Vec<Case>,
}

/// The contract this binary was built with.
pub fn bundled() -> Result<Contract> {
    const TEXT: &str = include_str!("../../tests/hosts/conformance.json");
    serde_json::from_str(TEXT).context("parsing the bundled conformance contract")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_contract_parses_and_covers_what_the_matrix_covered() {
        let contract = bundled().expect("the bundled contract parses");
        assert!(
            contract.cases.len() >= 26,
            "the contract has fewer cases than the matrix it replaces: {}",
            contract.cases.len()
        );
        // Every role the spec lists must actually be exercised, or the file
        // documents a capability nothing tests.
        for role in Role::all() {
            assert!(
                contract.cases.iter().any(|c| c.subject == role),
                "no case uses the {role:?} role"
            );
        }
    }
}
