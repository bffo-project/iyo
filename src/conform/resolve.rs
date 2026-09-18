//! Compiling the contract against one manifest.
//!
//! One resolver, four consumers, exactly as the manifest itself has one
//! producer and five adapters. Four harnesses each deciding what "a
//! dir-layout term" means would be four chances to disagree, and the file
//! exists so that they cannot.

use super::cases::{Case, Contract, Expect, Role};
use crate::negotiate;
use crate::render::manifest::{Manifest, NamespaceEntry};

/// A case with a concrete request in it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Resolved {
    pub name: String,
    /// The role that produced this case (`--json` must carry it,
    /// so a machine reader can tell which check a case came from without
    /// re-deriving it from the name).
    pub role: Role,
    pub group: String,
    /// The mount this subject came from, for grouping in the report. Always
    /// the namespace `resolve` walked in, even when the subject itself
    /// resolves inside a release mounted under it.
    pub namespace: String,
    /// Site-root-relative request path.
    pub path: String,
    pub accept: Option<String>,
    pub query: Option<String>,
    pub expect: ResolvedExpect,
    /// The `Link` header a negotiated response must carry, for `serve` and
    /// `redirect` expectations. The contract this replaced asserted three of
    /// these byte for byte; a role-based file cannot write the header
    /// literally, because it contains resolved paths, so it is computed
    /// here instead. This is what caught a release's `describedby` pointing
    /// at its parent's `llms.txt` (commit 125bf97) -- a check worth keeping
    /// alive across the rewrite, not dropping for convenience.
    pub expected_link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResolvedExpect {
    Serve {
        media_type: String,
        /// The canonical document URL the response body must contain --
        /// what `rel="canonical"` actually carries, which is byte-identical
        /// to the bare identity IRI for a flat-layout term and carries a
        /// trailing slash for a dir-layout one. See `canonical_url`.
        body_contains: String,
        /// The convention's explicit `Cache-Control`, from the manifest --
        /// `entry.cache_control`, which is already `snapshot_cache_control`
        /// for a subject inside a release (see `subject`'s doc comment).
        /// Without this, the gate could not tell the immutable release
        /// policy from the mutable latest one; it could only ask whether a
        /// `Cache-Control` was sent at all.
        cache_control: String,
        /// Output-root-relative path (no leading slash) the resolver would
        /// serve this from, e.g. `vocab/Widget.html`. Compared by the Rust
        /// harness, which has no socket and no body to check; the other
        /// harnesses ignore it.
        file: String,
    },
    Redirect {
        location: String,
        status: u16,
        /// See `Serve::cache_control`.
        cache_control: String,
    },
    File {
        media_type: String,
        /// See `Serve::cache_control`.
        cache_control: String,
    },
    Absent,
}

impl ResolvedExpect {
    /// The convention's explicit `Cache-Control`, to check against every kind
    /// except `Absent` -- a 404 is not a negotiated response and has no
    /// manifest-declared policy to compare against.
    pub fn cache_control(&self) -> Option<&str> {
        match self {
            ResolvedExpect::Serve { cache_control, .. }
            | ResolvedExpect::Redirect { cache_control, .. }
            | ResolvedExpect::File { cache_control, .. } => Some(cache_control.as_str()),
            ResolvedExpect::Absent => None,
        }
    }
}

/// The outcome of compiling a contract: what runs, and what did not.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    /// The version of this document's shape. `--cases` writes this file for
    /// the two local harnesses to read, so it is an interface like any
    /// other `--json` document.
    pub schema_version: &'static str,
    pub cases: Vec<Resolved>,
    /// Cases whose role found no subject. A run with any of these fails: a
    /// case that silently did not run is worse than one that is missing,
    /// because it reports success.
    pub unresolved: Vec<String>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            schema_version: crate::model::SCHEMA_VERSION,
            cases: Vec::new(),
            unresolved: Vec::new(),
        }
    }
}

pub fn resolve(contract: &Contract, manifest: &Manifest) -> Plan {
    let mut plan = Plan::default();
    for case in &contract.cases {
        let before = plan.cases.len();
        for ns in &manifest.namespaces {
            if let Some(resolved) = one(case, ns, manifest) {
                plan.cases.push(resolved);
            }
        }
        if plan.cases.len() == before {
            plan.unresolved.push(case.name.clone());
        }
    }
    plan
}

/// One case against one namespace, or `None` when the role finds no subject
/// there.
///
/// Everything after `subject()` is computed from the entry it returned, never
/// from `ns`: `ns` is only the namespace `resolve` is iterating, and for
/// `release-term` the subject actually lives in the snapshot entry, not in
/// `ns` itself. Reaching for `ns` here is the mistake commits 6913a01 and
/// 125bf97 fixed in `serve.rs` -- right about the paths by luck, wrong about
/// everything the release owns.
fn one(case: &Case, ns: &NamespaceEntry, manifest: &Manifest) -> Option<Resolved> {
    let role = case.subject;
    let (entry, path, iri) = subject(role, ns, manifest)?;
    let (expect, expected_link) = match &case.expect {
        Expect::Serve { media_type } => {
            // `serve` means the representation a namespace answers with at
            // the IRI itself, which is its default type. A case naming any
            // other type is a redirect case wearing the wrong kind: the
            // file and the `Link` header below are the default
            // representation's either way, so resolving it would judge a
            // reply against a type that file will never carry, and the case
            // could not pass against a correct host. Refusing it here puts
            // the case in `unresolved`, which fails the run out loud.
            if media_type != &entry.default_type {
                return None;
            }
            let local = path
                .strip_prefix(entry.mount.as_str())?
                .trim_end_matches('/');
            let reps = crate::adapter::term_representations(&entry);
            let chosen = reps
                .iter()
                .find(|r| r.media_type == entry.default_type)
                .copied()
                .or_else(|| reps.first().copied())?;
            let file = crate::adapter::term_file_path(&entry, local, chosen)
                .trim_start_matches('/')
                .to_owned();
            (
                ResolvedExpect::Serve {
                    media_type: media_type.clone(),
                    body_contains: iri,
                    cache_control: entry.cache_control.clone(),
                    file,
                },
                Some(negotiate::link_header(manifest, &entry, local, chosen)),
            )
        }
        Expect::Redirect { media_type } => {
            let local = path
                .strip_prefix(entry.mount.as_str())?
                .trim_end_matches('/');
            let rep = crate::adapter::term_representations(&entry)
                .into_iter()
                .find(|r| &r.media_type == media_type)?;
            (
                ResolvedExpect::Redirect {
                    location: negotiate::public_url(&entry, local, rep),
                    status: entry.status_code,
                    cache_control: entry.cache_control.clone(),
                },
                Some(negotiate::link_header(manifest, &entry, local, rep)),
            )
        }
        Expect::File { media_type } => (
            ResolvedExpect::File {
                media_type: media_type.clone(),
                cache_control: entry.cache_control.clone(),
            },
            None,
        ),
        Expect::Absent => (ResolvedExpect::Absent, None),
    };
    Some(Resolved {
        name: case.name.clone(),
        role,
        group: case.group.clone(),
        namespace: ns.mount.clone(),
        path,
        accept: case.accept.clone(),
        query: case.query.clone(),
        expect,
        expected_link,
    })
}

/// A term's local name, resolved to the namespace entry it belongs to, the
/// request path, and the URL a `serve` case's body must be checked against.
/// Shared by `term` and `dir-term`, which differ only in which list they
/// read the local name from.
fn term_subject(
    manifest: &Manifest,
    ns: &NamespaceEntry,
    local: &str,
) -> (NamespaceEntry, String, String) {
    let entry = ns.clone();
    let path = crate::adapter::term_request_path(&entry, local);
    let iri = canonical_url(manifest, &entry, local)
        .unwrap_or_else(|| format!("{}{local}", entry.iri_base));
    (entry, path, iri)
}

/// The absolute URL a term's HTML page actually carries in
/// `rel="canonical"` -- byte-identical to what `negotiate::link_header`
/// computes for the same relation, and to what `site.rs::term_url` writes
/// into the template that emits it.
///
/// For a flat-layout term this equals the bare identity IRI (`iri_base +
/// local`). For a dir-layout term (one called `index` or `shapes`, or a
/// case-fold collision) it does not: `term_url`'s own doc
/// comment says the trailing slash is "what makes the term IRI and the
/// document URL line up" with the *document*, but the RDF subject and
/// `rel="cite-as"` stay bare (`negotiate::link_header` computes `cite-as`
/// straight from `ns.iri_base`, with no directory suffix) -- `cite-as`
/// names the resource, `canonical` names where this particular page lives.
/// `cite-as` is also HTTP-header-only unless the manifest turns
/// `page.cite_as` on, so for a `serve` case's body check -- which can only
/// read what is actually rendered into the page -- `canonical`'s value is
/// the one reliably present to match against, and `None` here (no
/// representations at all) falls back to the bare identity IRI.
fn canonical_url(manifest: &Manifest, ns: &NamespaceEntry, local: &str) -> Option<String> {
    let reps = crate::adapter::term_representations(ns);
    let html = reps
        .iter()
        .find(|r| r.media_type == ns.default_type)
        .copied()
        .or_else(|| reps.first().copied())?;
    let origin = manifest.site_root.trim_end_matches('/');
    Some(format!(
        "{origin}{}",
        negotiate::public_url(ns, local, html)
    ))
}

/// The namespace entry a role's subject actually lives in, the request path
/// within it, and the identity IRI that path denotes. Deterministic, so two
/// runs of one release produce the same case list.
///
/// The entry is `ns.clone()` for every role except `release-term`, whose
/// subject lives inside the snapshot `negotiate::snapshot_entry` produces:
/// the same function the renderer and `negotiate::target` use, so this
/// cannot describe a release differently than the release describes itself.
fn subject(
    role: Role,
    ns: &NamespaceEntry,
    manifest: &Manifest,
) -> Option<(NamespaceEntry, String, String)> {
    match role {
        Role::Term => Some(term_subject(manifest, ns, ns.terms.first()?)),
        Role::DirTerm => Some(term_subject(manifest, ns, ns.dir_terms.first()?)),
        Role::Sibling => {
            // "a term's representation named directly (L.ttl)":
            // the turtle sibling specifically, not whichever non-HTML
            // representation happens to sort first. `Rep::produced()`
            // always lists markdown before turtle, so "first non-empty
            // suffix" silently picked `.md` on every manifest this tool
            // builds -- a role whose case in the bundled contract always
            // expects `text/turtle` and could therefore never pass.
            let local = ns.terms.first()?;
            let entry = ns.clone();
            let rep = crate::adapter::term_representations(&entry)
                .into_iter()
                .find(|r| r.media_type == "text/turtle")?;
            let path = negotiate::public_url(&entry, local, rep);
            let iri = format!("{}{local}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::Namespace => {
            let entry = ns.clone();
            let path = entry.mount.clone();
            let iri = entry.iri_base.clone();
            Some((entry, path, iri))
        }
        Role::NestedNamespace => {
            // A mount lying under this one. `/ontology/shapes/` under
            // `/ontology/`, which a naive rule resolves as a term called
            // `shapes` of the parent.
            let entry = manifest
                .namespaces
                .iter()
                .find(|n| n.mount != ns.mount && n.mount.starts_with(&ns.mount))?
                .clone();
            let path = entry.mount.clone();
            let iri = entry.iri_base.clone();
            Some((entry, path, iri))
        }
        Role::ReleaseTerm => {
            let version = ns.versions.first()?;
            let local = ns.terms.first()?;
            let entry = negotiate::snapshot_entry(ns, version);
            let path = crate::adapter::term_request_path(&entry, local);
            let iri = canonical_url(manifest, &entry, local)
                .unwrap_or_else(|| format!("{}{local}", entry.iri_base));
            Some((entry, path, iri))
        }
        Role::EmptyNamespace => {
            if !ns.terms.is_empty() {
                return None;
            }
            let entry = ns.clone();
            let path = format!("{}{ABSENT_NAME}", entry.mount);
            let iri = format!("{}{ABSENT_NAME}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::ReservedSegment => {
            // `reserved` merges nested-namespace segments with release
            // segments -- both stop a naive resolver from treating the path
            // as a term. Release segments already have three dedicated
            // roles (`ReleaseTerm`, `AbsentRelease`, `AbsentReleaseName`),
            // so letting this one land on a release segment too would make
            // the role vocabulary ambiguous about which check actually ran.
            // Excluding versions keeps this role on the nested-namespace
            // collision it exists for -- do not simplify the filter back
            // out to a bare `.first()`: on this fixture that silently
            // steers it onto the release segment instead.
            let segment = ns
                .reserved
                .iter()
                .find(|r| !ns.versions.iter().any(|v| &v.segment == *r))?;
            let entry = ns.clone();
            // The convention does not define which of 404/301/200 the file
            // layer answers for the bare segment (`/vocab/shapes`), so the
            // case built on this role does not ask for it. It DOES define that
            // the segment must not resolve as a term of its parent, which is
            // what `reserved` exists for -- so the request asks for the
            // segment's turtle sibling instead, which must not resolve either.
            // Requires a namespace whose terms actually have a turtle
            // representation, same as `Sibling`.
            let rep = crate::adapter::term_representations(&entry)
                .into_iter()
                .find(|r| r.media_type == "text/turtle")?;
            let suffix = rep.suffix.as_deref().unwrap_or_default();
            let path = format!("{}{segment}{suffix}", entry.mount);
            let iri = format!("{}{segment}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::SubTermPath => {
            let local = ns.terms.first()?;
            let entry = ns.clone();
            let path = format!("{}{local}/extra", entry.mount);
            let iri = format!("{}{local}/extra", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::AbsentName => {
            // Asserted absent rather than assumed: if a release ever mints a
            // term with this name the role fails instead of testing a
            // falsehood.
            if ns.terms.iter().any(|t| t == ABSENT_NAME) {
                return None;
            }
            let entry = ns.clone();
            let path = format!("{}{ABSENT_NAME}", entry.mount);
            let iri = format!("{}{ABSENT_NAME}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::AbsentRelease => {
            if ns.versions.iter().any(|v| v.segment == ABSENT_RELEASE) {
                return None;
            }
            let local = ns.terms.first()?;
            let entry = ns.clone();
            let path = format!("{}{ABSENT_RELEASE}/{local}", entry.mount);
            let iri = format!("{}{ABSENT_RELEASE}/{local}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::CaseVariant => {
            // A term whose case-fold twin is not itself a term. Skipping the
            // ones where it is, is the whole point: BFFO publishes both
            // `FormatVersion` and `formatVersion`.
            let local = ns.terms.iter().find(|t| {
                let variant = flip_first(t);
                variant != **t && !ns.terms.contains(&variant)
            })?;
            let variant = flip_first(local);
            let entry = ns.clone();
            let path = format!("{}{variant}", entry.mount);
            let iri = format!("{}{variant}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::SiblingUnpublished => {
            let published: Vec<&str> = ns
                .representations
                .iter()
                .filter_map(|r| r.suffix.as_deref())
                .filter(|s| !s.is_empty())
                .collect();
            let suffix = [".nt", ".rdf", ".n3"]
                .into_iter()
                .find(|c| !published.contains(c))?;
            let local = ns.terms.first()?;
            let entry = ns.clone();
            let path = format!("{}{local}{suffix}", entry.mount);
            let iri = format!("{}{local}", entry.iri_base);
            Some((entry, path, iri))
        }
        Role::AbsentReleaseName => {
            let version = ns.versions.first()?;
            if ns.terms.iter().any(|t| t == ABSENT_NAME) {
                return None;
            }
            Some((
                ns.clone(),
                format!("{}{}/{ABSENT_NAME}", ns.mount, version.segment),
                format!("{}{}/{ABSENT_NAME}", ns.iri_base, version.segment),
            ))
        }
    }
}

/// A name no release mints. A literal rather than something generated, so a
/// failure message reads the same on every run and is recognisable in a log.
pub const ABSENT_NAME: &str = "iyo-absent-name";

/// A version segment no release publishes.
pub const ABSENT_RELEASE: &str = "0.0.0-iyo-absent";

/// The same local name with the case of its first letter flipped.
fn flip_first(local: &str) -> String {
    let mut chars = local.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            first.to_lowercase().collect::<String>() + chars.as_str()
        }
        Some(first) if first.is_lowercase() => {
            first.to_uppercase().collect::<String>() + chars.as_str()
        }
        _ => local.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conform::cases;

    /// Builds the manifest of `testdata/mini`, the same fixture
    /// `tests/negotiate.rs` uses, so both are talking about one vocabulary.
    fn mini() -> crate::render::manifest::Manifest {
        let base = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = crate::load::expand_inputs(&["testdata/mini".to_owned()], &base).unwrap();
        let store = crate::load::load(&paths).unwrap();
        let registry = crate::profile::Registry::built_in().unwrap();
        let release = crate::build::build(&store, &registry).unwrap();
        let mut config = crate::config::Config::implicit();
        config.site.base_url = "https://example.org/".to_owned();
        let plan = crate::site::Plan::new(&release, &config);
        let ctx = crate::render::Ctx {
            release: &release,
            store: &store,
            plan: &plan,
            config: &config,
            changes: None,
        };
        crate::render::manifest::build(&ctx)
    }

    #[test]
    fn a_term_resolves_to_one_subject_per_namespace() {
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "t".to_owned(),
                subject: cases::Role::Term,
                accept: Some("text/turtle".to_owned()),
                query: None,
                expect: cases::Expect::Redirect {
                    media_type: "text/turtle".to_owned(),
                },
                group: "negotiation".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        assert!(plan.unresolved.is_empty(), "{:?}", plan.unresolved);
        // One per namespace that has terms, not one per term: conformance is
        // whether the rule holds, coverage is what `probe` is for.
        let with_terms = manifest
            .namespaces
            .iter()
            .filter(|n| !n.terms.is_empty())
            .count();
        assert_eq!(plan.cases.len(), with_terms);

        let one = plan
            .cases
            .iter()
            .find(|c| c.path == "/vocab/Thing")
            .expect("a term of /vocab/");
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert_eq!(one.role, cases::Role::Term);
        assert_eq!(
            one.expect,
            ResolvedExpect::Redirect {
                location: "/vocab/Thing.ttl".to_owned(),
                status: 303,
                cache_control: vocab.cache_control.clone(),
            }
        );
    }

    #[test]
    fn a_sibling_role_always_names_the_turtle_representation() {
        // The contract defines `sibling` as "a term's representation named
        // directly (L.ttl)". `Rep::produced()` always lists markdown before
        // turtle, so picking "whichever non-HTML representation sorts
        // first" resolved to `.md` on every manifest this tool builds --
        // and the bundled contract's only `sibling` case expects
        // `text/turtle`, which that path could then never satisfy.
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "sib".to_owned(),
                subject: cases::Role::Sibling,
                accept: Some("text/turtle".to_owned()),
                query: None,
                expect: cases::Expect::File {
                    media_type: "text/turtle".to_owned(),
                },
                group: "negotiation".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        assert!(plan.unresolved.is_empty(), "{:?}", plan.unresolved);
        let one = plan
            .cases
            .iter()
            .find(|c| c.namespace == "/vocab/")
            .expect("a sibling case for /vocab/");
        assert_eq!(one.path, "/vocab/Thing.ttl");
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert_eq!(
            one.expect,
            ResolvedExpect::File {
                media_type: "text/turtle".to_owned(),
                cache_control: vocab.cache_control.clone(),
            }
        );
    }

    /// A `serve` case naming anything but the namespace's default type
    /// cannot be satisfied: the file and the `Link` header a serve case
    /// resolves to are the default representation's, so the reply would be
    /// judged against a type that file never carries. It must land in
    /// `unresolved`, which fails the run, rather than resolving into a case
    /// no correct host could pass.
    #[test]
    fn a_serve_case_for_a_type_the_namespace_does_not_default_to_is_refused() {
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "turtle at the iri".to_owned(),
                subject: cases::Role::Term,
                accept: Some("text/turtle".to_owned()),
                query: None,
                expect: cases::Expect::Serve {
                    media_type: "text/turtle".to_owned(),
                },
                group: "negotiation".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        assert!(plan.cases.is_empty(), "{:?}", plan.cases);
        assert_eq!(plan.unresolved, vec!["turtle at the iri".to_owned()]);
    }

    #[test]
    fn a_serve_case_carries_the_identity_iri_the_body_must_name() {
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "page".to_owned(),
                subject: cases::Role::Term,
                accept: Some("text/html".to_owned()),
                query: None,
                expect: cases::Expect::Serve {
                    media_type: "text/html".to_owned(),
                },
                group: "negotiation".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        let one = plan
            .cases
            .iter()
            .find(|c| c.path == "/vocab/Thing")
            .unwrap();
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        // 200 text/html is what a catch-all route answers for every unknown
        // path. The body naming the IRI is the only thing that separates the
        // right page from an application shell.
        assert_eq!(
            one.expect,
            ResolvedExpect::Serve {
                media_type: "text/html".to_owned(),
                body_contains: "https://example.org/vocab/Thing".to_owned(),
                cache_control: vocab.cache_control.clone(),
                file: "vocab/Thing.html".to_owned(),
            }
        );
    }

    /// A dir-layout term's `body_contains` must be the URL its page's own
    /// `rel="canonical"` link actually carries, trailing slash included --
    /// not the bare identity IRI `rel="cite-as"` uses. For a dir-layout term,
    /// `term_url`'s own doc comment says the trailing slash on a dir-layout
    /// HTML URL is "what makes the term IRI and the document URL line up" with
    /// the document, but `negotiate::link_header` computes `cite-as` straight
    /// from `ns.iri_base` with no directory suffix, so the two relations name
    /// the resource and the document differently for exactly this layout.
    /// `cite-as` is also HTTP-header-only unless the manifest turns
    /// `page.cite_as` on, so it is not reliably in the body to check against.
    /// Getting this wrong means every `serve` case for a dir-layout term fails
    /// against a real page that is in fact correct -- caught only once the
    /// anchored body check (IMPORTANT 5) stopped a bare substring match from
    /// tolerating the extra slash silently.
    #[test]
    fn a_dir_term_serve_case_names_the_canonical_document_url_not_the_bare_identity_iri() {
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "dir page".to_owned(),
                subject: cases::Role::DirTerm,
                accept: Some("text/html".to_owned()),
                query: None,
                expect: cases::Expect::Serve {
                    media_type: "text/html".to_owned(),
                },
                group: "negotiation".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        assert!(plan.unresolved.is_empty(), "{:?}", plan.unresolved);
        let one = plan
            .cases
            .iter()
            .find(|c| c.name == "dir page")
            .expect("a dir-term serve case");
        let ResolvedExpect::Serve { body_contains, .. } = &one.expect else {
            panic!("expected a Serve expectation, got {:?}", one.expect);
        };
        assert!(
            body_contains.ends_with('/'),
            "a dir-layout term's canonical document URL carries a trailing \
             slash: {body_contains}"
        );
        // The path a request would be sent to, and the URL its response body
        // must name, describe the same resource and must agree once origin
        // and trailing slash are normalised away.
        assert_eq!(format!("https://example.org{}", one.path), *body_contains);
    }

    fn release_term_case(expect: cases::Expect) -> cases::Contract {
        cases::Contract {
            cases: vec![cases::Case {
                name: "r".to_owned(),
                subject: cases::Role::ReleaseTerm,
                accept: None,
                query: None,
                expect,
                group: "releases".to_owned(),
            }],
        }
    }

    /// Commit 6913a01 fixed the resolver treating a release as passed
    /// through; 125bf97 then found that even once resolved, `describedby`
    /// still pointed at the parent's `llms.txt`, because the header was
    /// computed from the namespace instead of the snapshot the release
    /// actually is. `subject()` must resolve a `release-term` inside the
    /// snapshot entry, not the parent namespace: everything computed from it
    /// afterwards -- the request path, the identity IRI, and the `Link`
    /// header -- has to describe the release, not the moving namespace.
    #[test]
    fn a_release_term_resolves_within_its_own_snapshot_not_the_parent() {
        let manifest = mini();
        let plan = resolve(
            &release_term_case(cases::Expect::Serve {
                media_type: "text/html".to_owned(),
            }),
            &manifest,
        );
        assert!(plan.unresolved.is_empty(), "{:?}", plan.unresolved);
        let one = plan
            .cases
            .iter()
            .find(|c| c.name == "r")
            .expect("one release-term case");

        // The subject lives under the version segment, not at the mount the
        // report groups it by.
        assert_eq!(one.path, "/vocab/0.1.0/Thing");
        assert_eq!(one.namespace, "/vocab/");
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        // The immutable snapshot policy, not the mutable one the moving
        // namespace itself carries -- the exact substitution CRITICAL 2
        // exists to hold the gate to: `iyo serve` sending `LATEST_CACHE`
        // inside a release would be invisible to a check that only asked
        // whether *some* Cache-Control was present.
        assert_eq!(
            one.expect,
            ResolvedExpect::Serve {
                media_type: "text/html".to_owned(),
                body_contains: "https://example.org/vocab/0.1.0/Thing".to_owned(),
                cache_control: vocab.snapshot_cache_control.clone(),
                file: "vocab/0.1.0/Thing.html".to_owned(),
            }
        );
        assert_ne!(
            vocab.cache_control, vocab.snapshot_cache_control,
            "the assertion above is meaningless if these ever match"
        );

        // Computed from the snapshot: `describedby` is a bare `llms.txt`,
        // sitting beside `Thing` inside the release. Computed from the
        // parent it would climb out with `../llms.txt` instead, naming the
        // moving namespace's agent index rather than the release's own --
        // exactly the defect 125bf97 fixed.
        let link = one
            .expected_link
            .as_deref()
            .expect("a serve case carries a Link header");
        assert!(
            link.contains("<llms.txt>; rel=\"describedby\""),
            "describedby did not name the release's own llms.txt: {link}"
        );
        assert!(
            !link.contains("../llms.txt"),
            "describedby climbed out of the release into the parent: {link}"
        );
        assert!(link.contains("<https://example.org/vocab/0.1.0/Thing>; rel=\"canonical\""));
        assert!(link.contains("<https://example.org/vocab/0.1.0/Thing>; rel=\"cite-as\""));
    }

    /// The same check on the redirect side: the sibling URL and its `Link`
    /// header both have to stay inside the release segment.
    #[test]
    fn a_release_terms_redirect_stays_inside_the_release() {
        let manifest = mini();
        let plan = resolve(
            &release_term_case(cases::Expect::Redirect {
                media_type: "text/turtle".to_owned(),
            }),
            &manifest,
        );
        let one = plan
            .cases
            .iter()
            .find(|c| c.name == "r")
            .expect("one release-term case");
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert_eq!(
            one.expect,
            ResolvedExpect::Redirect {
                location: "/vocab/0.1.0/Thing.ttl".to_owned(),
                status: 303,
                cache_control: vocab.snapshot_cache_control.clone(),
            }
        );
        let link = one
            .expected_link
            .as_deref()
            .expect("a redirect case carries a Link header");
        assert!(link.contains("<llms.txt>; rel=\"describedby\""));
        assert!(!link.contains("../llms.txt"));
    }

    /// A file asked for by name is not a negotiation, and a 404 is not one
    /// either: neither carries a `Link` header to check.
    #[test]
    fn file_and_absent_expectations_carry_no_expected_link() {
        let manifest = mini();

        let file_plan = resolve(
            &cases::Contract {
                cases: vec![cases::Case {
                    name: "sibling".to_owned(),
                    subject: cases::Role::Sibling,
                    accept: Some("text/turtle".to_owned()),
                    query: None,
                    expect: cases::Expect::File {
                        media_type: "text/turtle".to_owned(),
                    },
                    group: "negotiation".to_owned(),
                }],
            },
            &manifest,
        );
        assert!(file_plan.cases.iter().all(|c| c.expected_link.is_none()));
        assert!(!file_plan.cases.is_empty());

        let absent_plan = resolve(
            &cases::Contract {
                cases: vec![cases::Case {
                    name: "empty".to_owned(),
                    subject: cases::Role::EmptyNamespace,
                    accept: Some("text/turtle".to_owned()),
                    query: None,
                    expect: cases::Expect::Absent,
                    group: "negative perimeter".to_owned(),
                }],
            },
            &manifest,
        );
        let one = absent_plan
            .cases
            .iter()
            .find(|c| c.name == "empty")
            .expect("the empty namespace");
        assert_eq!(one.path, "/vocab/shapes/iyo-absent-name");
        assert_eq!(one.expect, ResolvedExpect::Absent);
        assert!(one.expected_link.is_none());
    }

    #[test]
    fn the_negative_roles_name_things_the_manifest_says_are_not_terms() {
        let manifest = mini();
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();

        let path = |role| subject(role, vocab, &manifest).map(|(_, p, _)| p);

        assert_eq!(
            path(cases::Role::AbsentName).as_deref(),
            Some("/vocab/iyo-absent-name")
        );
        assert_eq!(
            path(cases::Role::AbsentRelease).as_deref(),
            Some("/vocab/0.0.0-iyo-absent/Thing")
        );
        assert_eq!(
            path(cases::Role::SubTermPath).as_deref(),
            Some("/vocab/Thing/extra")
        );
        assert_eq!(
            path(cases::Role::SiblingUnpublished).as_deref(),
            Some("/vocab/Thing.nt")
        );
        // /vocab/ reserves `shapes` (and `0.1.0`, the release segment, which
        // the filter excludes). The convention does not define which of
        // 404/301/200 the file layer answers for the bare segment, so the case
        // built on this role asks for its turtle sibling instead -- which the
        // namespace must not resolve as `shapes`'s own representation either.
        assert_eq!(
            path(cases::Role::ReservedSegment).as_deref(),
            Some("/vocab/shapes.ttl")
        );
    }

    /// `--json` must carry `role`, or a machine reader cannot tell
    /// which check produced a case without re-deriving it from the name.
    #[test]
    fn a_resolved_case_carries_the_role_that_produced_it() {
        let manifest = mini();
        let contract = cases::Contract {
            cases: vec![cases::Case {
                name: "a".to_owned(),
                subject: cases::Role::AbsentName,
                accept: Some("text/turtle".to_owned()),
                query: None,
                expect: cases::Expect::Absent,
                group: "negative perimeter".to_owned(),
            }],
        };
        let plan = resolve(&contract, &manifest);
        assert!(!plan.cases.is_empty());
        assert!(plan.cases.iter().all(|c| c.role == cases::Role::AbsentName));
    }

    /// BFFO publishes both `FormatVersion` and `formatVersion`, which is the
    /// collision the case-fold rule exists for. Asserting the second does not
    /// resolve would assert a falsehood, so the role must find a term whose
    /// twin is absent.
    #[test]
    fn a_case_variant_is_skipped_when_the_variant_is_itself_a_term() {
        let mut manifest = mini();
        let ns = manifest
            .namespaces
            .iter_mut()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        ns.terms = vec!["Widget".to_owned(), "widget".to_owned()];
        let ns = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert!(subject(cases::Role::CaseVariant, ns, &manifest).is_none());

        let mut manifest = mini();
        let ns = manifest
            .namespaces
            .iter_mut()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        ns.terms = vec!["Widget".to_owned()];
        let ns = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert_eq!(
            subject(cases::Role::CaseVariant, ns, &manifest)
                .map(|(_, p, _)| p)
                .as_deref(),
            Some("/vocab/widget")
        );
    }

    /// `absent-release-name` is distinct from `absent-release`: the release
    /// segment itself is real, but the name inside it is not. It guards the
    /// code path `negotiate::resolve` gained in 6913a01 when it learned to
    /// walk release paths -- an unminted name inside a real release must
    /// still 404, not fall through to whatever the release does publish.
    #[test]
    fn absent_release_name_resolves_inside_a_real_release_but_not_without_one() {
        let manifest = mini();
        let vocab = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .unwrap();
        assert_eq!(
            subject(cases::Role::AbsentReleaseName, vocab, &manifest)
                .map(|(_, p, _)| p)
                .as_deref(),
            Some("/vocab/0.1.0/iyo-absent-name")
        );

        // /vocab/shapes/ has no versions, so there is no release to be
        // absent from a name inside.
        let shapes = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/shapes/")
            .unwrap();
        assert!(subject(cases::Role::AbsentReleaseName, shapes, &manifest).is_none());
    }
}
