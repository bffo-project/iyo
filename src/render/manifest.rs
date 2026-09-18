//! The host-neutral negotiation manifest.
//!
//! This is the artefact the design is built around: a declarative map from a
//! namespace and a term to representation URLs by media type, from which host
//! adapters are compiled by pure functions. Existing generators emit Apache
//! rewrite rules instead, which only Apache and w3id can consume.
//!
//! It is deliberately a superset of a DCMI-style resolver configuration, so
//! that such an entry can be projected from it without loss: `suffix: null`
//! means the namespace-level file only, which is that schema's `append: none`,
//! and the order of `representations` is the negotiation tie-break.

use super::Ctx;
use crate::site::Rep;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Representation {
    pub media_type: String,
    /// Appended to the term URL. `null` means this media type exists only at
    /// the namespace level.
    pub suffix: Option<String>,
    pub namespace_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceEntry {
    pub id: String,
    pub kind: String,
    /// What the RDF mints. Byte-exact, scheme included; never rewritten.
    pub iri_base: String,
    /// Where the documents are served.
    pub doc_base: String,
    pub mount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver_prefix: Option<String>,
    pub layout: String,
    /// Local names served from a directory although the namespace layout is
    /// flat: a name the site itself owns, or one that folds together with
    /// another when case is ignored. An adapter that ignored these
    /// would route exactly the terms those rules rescued to the wrong file.
    pub dir_terms: Vec<String>,
    /// `strict` answers 404 for a local name not in `terms`.
    pub resolver_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    pub default_type: String,
    pub status_code: u16,
    pub cache_max_age: u32,
    pub representations: Vec<Representation>,
    /// Local names this namespace publishes, sorted. A resolver configured
    /// strictly answers 404 for anything else.
    pub terms: Vec<String>,
    /// Path segments under this namespace that are documents, not terms.
    pub reserved: Vec<String>,
    pub llms_txt: String,
    /// Releases published under this namespace. A resolver sends the version
    /// IRI to the snapshot rather than to the newest state, which is the
    /// difference between a citation that keeps working and one that drifts.
    pub versions: Vec<crate::version::Snapshot>,
    /// What a host should send for this namespace's own files, and for
    /// anything under a snapshot.
    pub cache_control: String,
    pub snapshot_cache_control: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub convention: String,
    pub generator: BTreeMap<String, String>,
    pub site_root: String,
    pub namespaces: Vec<NamespaceEntry>,
}

pub fn build(ctx: &Ctx<'_>) -> Manifest {
    let lang = ctx.lang();
    let namespaces = ctx
        .plan
        .namespaces
        .iter()
        .map(|ns| {
            let doc = ns
                .document
                .as_deref()
                .and_then(|iri| ctx.release.document(iri));
            let representations = Rep::produced()
                .into_iter()
                .map(|rep| Representation {
                    media_type: rep.media_type().to_owned(),
                    suffix: match rep {
                        Rep::Html => Some(String::new()),
                        _ => Some(rep.extension().to_owned()),
                    },
                    namespace_file: ctx.plan.document_path(ns, rep),
                })
                .collect();
            NamespaceEntry {
                id: ns
                    .prefix
                    .clone()
                    .unwrap_or_else(|| ns.mount.trim_end_matches('/').replace('/', "-")),
                kind: doc
                    .map(|d| format!("{:?}", d.kind).to_lowercase())
                    .unwrap_or_else(|| "document".to_owned()),
                iri_base: ns.iri.clone(),
                doc_base: format!("{}{}", ctx.plan.base_url, ns.mount),
                mount: format!("/{}", ns.mount),
                resolver_prefix: ns.resolver_prefix.clone(),
                layout: match ns.style {
                    crate::site::UrlStyle::Flat => "flat".to_owned(),
                    crate::site::UrlStyle::Dir => "dir".to_owned(),
                },
                dir_terms: {
                    let mut d: Vec<String> = ctx
                        .release
                        .local_terms()
                        .filter(|t| t.namespace == ns.iri)
                        .map(|t| t.local_name.clone())
                        .filter(|l| {
                            crate::site::Plan::is_reserved_stem(ns, l) || ns.cased.contains(l)
                        })
                        .collect();
                    d.sort();
                    d
                },
                resolver_type: "strict".to_owned(),
                prefix: ns.prefix.clone(),
                title: doc.map(|d| d.display(lang).to_owned()),
                version: doc.and_then(|d| d.header.version_info.clone()),
                version_iri: doc.and_then(|d| d.header.version_iri.clone()),
                status: doc.and_then(|d| d.header.status.clone()),
                licence: doc.and_then(|d| d.header.license.clone()),
                default_type: Rep::Html.media_type().to_owned(),
                // 303 is the linked-data recommendation for slash namespaces.
                status_code: 303,
                cache_max_age: 86400,
                representations,
                terms: {
                    let mut t: Vec<String> = ctx
                        .release
                        .local_terms()
                        .filter(|t| t.namespace == ns.iri)
                        .map(|t| t.local_name.clone())
                        .collect();
                    t.sort();
                    t
                },
                reserved: {
                    // A snapshot segment is a release, not a term, and a
                    // resolver configured from this file has to know that
                    // before it answers 404 for everything unlisted.
                    let mut r = ns.reserved.clone();
                    r.extend(ctx.snapshots(ns).into_iter().map(|s| s.segment));
                    r.sort();
                    r.dedup();
                    r
                },
                llms_txt: ctx.plan.llms_url(ns),
                versions: ctx.snapshots(ns),
                cache_control: super::LATEST_CACHE.to_owned(),
                snapshot_cache_control: super::SNAPSHOT_CACHE.to_owned(),
            }
        })
        .collect();

    let mut generator = BTreeMap::new();
    generator.insert("name".to_owned(), env!("CARGO_PKG_NAME").to_owned());
    generator.insert("version".to_owned(), env!("CARGO_PKG_VERSION").to_owned());

    Manifest {
        convention: "iyo/1".to_owned(),
        generator,
        site_root: ctx.plan.base_url.clone(),
        namespaces,
    }
}
