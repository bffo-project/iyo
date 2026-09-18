//! Host adapters: deployable configuration compiled from the manifest.
//!
//! This is the part of the design the rest is arranged around: a host
//! configuration is a pure function of the manifest. Existing generators emit
//! Apache rewrite rules, which only Apache and w3id can consume, so moving a
//! vocabulary to another host means rewriting its resolution rules by hand
//! and hoping they still mean the same thing. Here the negotiation table is
//! the artefact and every host configuration is a projection of it.
//!
//! For that claim to be worth anything the projections have to be *pure
//! functions of the manifest*, so every adapter in this module takes
//! `&Manifest` and nothing else. It cannot reach the release, the store or
//! the configuration, which is what makes "the same table, five hosts" a fact
//! about the code rather than a description of it.
//!
//! What each host can and cannot do is documented rather than hidden
//! (`docs/output-convention.md`, "Host adapters compiled from the
//! manifest"): Apache matches `Accept` with a regex and cannot rank
//! q-values, Vercel matches headers but not q-values,
//! GitHub Pages cannot negotiate at all. An adapter that silently produced
//! worse behaviour than the convention describes would be the failure mode
//! this design exists to avoid, so each writes a `README` saying what it does
//! not do.

pub mod apache;
pub mod cloudflare;
pub mod dcmi;
pub mod github;
pub mod vercel;

use crate::render::manifest::{Manifest, NamespaceEntry, Representation};

/// A host `iyo` can compile the manifest for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Host {
    Cloudflare,
    Apache,
    Vercel,
    DcmiNs,
    GitHubPages,
}

impl Host {
    pub fn parse(value: &str) -> Option<Host> {
        match value {
            "cloudflare" => Some(Host::Cloudflare),
            "apache" => Some(Host::Apache),
            "vercel" => Some(Host::Vercel),
            "dcmi-ns" => Some(Host::DcmiNs),
            "github-pages" => Some(Host::GitHubPages),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Host::Cloudflare => "cloudflare",
            Host::Apache => "apache",
            Host::Vercel => "vercel",
            Host::DcmiNs => "dcmi-ns",
            Host::GitHubPages => "github-pages",
        }
    }

    pub const ALL: [Host; 5] = [
        Host::Cloudflare,
        Host::Apache,
        Host::Vercel,
        Host::DcmiNs,
        Host::GitHubPages,
    ];

    /// Everything the convention asks for that this host cannot do. Written
    /// into the adapter's own README so a publisher reads it before
    /// deploying, not after a conformance checker fails.
    pub fn degradations(self) -> &'static [&'static str] {
        match self {
            Host::Cloudflare => &[],
            Host::Apache => &[
                "`Accept` is matched with a regular expression, so q-values are not ranked: \
                 a client asking for `text/turtle;q=0.9, text/html;q=0.8` may be sent either.",
                "Per-term `Link` headers are not emitted; the HTML `<link>` elements are the floor.",
            ],
            Host::Vercel => &[
                "Header matching has no q-value ranking, so the order of the rules decides.",
                "`Link` headers are set per namespace, not per term.",
            ],
            Host::DcmiNs => &[
                "No per-term suffix: a resolver entry sends a term to one representation.",
                "No versioned paths and no anchor map until the upstream schema grows them.",
            ],
            Host::GitHubPages => &[
                "No negotiation of any kind: only the files are served.",
                "An IRI without an extension will not resolve; pair this with w3id or a resolver.",
            ],
        }
    }
}

/// Emit one host's files, as (path, content) pairs under `adapters/<host>/`.
pub fn emit(manifest: &Manifest, host: Host) -> Vec<(String, String)> {
    let files = match host {
        Host::Cloudflare => cloudflare::emit(manifest),
        Host::Apache => apache::emit(manifest),
        Host::Vercel => vercel::emit(manifest),
        Host::DcmiNs => dcmi::emit(manifest),
        Host::GitHubPages => github::emit(manifest),
    };
    let mut out: Vec<(String, String)> = files
        .into_iter()
        .map(|(name, body)| (format!("adapters/{}/{name}", host.as_str()), body))
        .collect();
    out.push((
        format!("adapters/{}/README.md", host.as_str()),
        readme(manifest, host),
    ));
    out
}

fn readme(manifest: &Manifest, host: Host) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "# Deploying to {}\n", host.as_str());
    let _ = writeln!(
        s,
        "Compiled from `manifest.json` by {} {}. Every file here is a pure function of that\n\
         manifest: edit the vocabulary or the site configuration and rebuild, do not edit these.\n",
        manifest
            .generator
            .get("name")
            .map(String::as_str)
            .unwrap_or("iyo"),
        manifest
            .generator
            .get("version")
            .map(String::as_str)
            .unwrap_or("")
    );
    let _ = writeln!(s, "Site root: {}\n", manifest.site_root);
    let _ = writeln!(s, "## Namespaces\n");
    let _ = writeln!(s, "| Mount | Terms | Releases |");
    let _ = writeln!(s, "| --- | --- | --- |");
    for ns in &manifest.namespaces {
        let versions = if ns.versions.is_empty() {
            "none".to_owned()
        } else {
            ns.versions
                .iter()
                .map(|v| v.segment.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(s, "| `{}` | {} | {versions} |", ns.mount, ns.terms.len());
    }
    s.push('\n');

    let degradations = host.degradations();
    let limits = limits(manifest, host);
    let _ = writeln!(s, "## What this host cannot do\n");
    if degradations.is_empty() && limits.is_empty() {
        let _ = writeln!(
            s,
            "Nothing is lost on this host: every rule in `manifest.json` is served as written.\n"
        );
    } else {
        for d in degradations {
            let _ = writeln!(s, "- {d}");
        }
        // Written verbatim, not prefixed: a limit may be a nested list.
        for l in &limits {
            let _ = writeln!(s, "{l}");
        }
        s.push('\n');
    }
    s
}

/// What this host cannot do **about this manifest**, as opposed to
/// `Host::degradations`, which is what it can never do.
///
/// A static list understates a loss exactly when it matters and is noise
/// when it does not: `dcmi-ns` handles a namespace of flat terms perfectly
/// and cannot express one where a single term falls back to the directory
/// layout, and only the manifest knows which this is.
///
/// Each string is a markdown list item written verbatim, so a limit can
/// carry a nested list of the namespaces it applies to.
fn limits(manifest: &Manifest, host: Host) -> Vec<String> {
    let mut out = Vec::new();
    if host == Host::DcmiNs {
        let affected: Vec<&NamespaceEntry> = manifest
            .namespaces
            .iter()
            .filter(|ns| !ns.dir_terms.is_empty())
            .collect();
        if !affected.is_empty() {
            out.push(
                "- This schema has one trailing-slash setting per namespace, so it cannot \
                 express a per-term layout. A resolver built from these entries sends the \
                 terms below to the namespace's own document instead of the term's page. \
                 The `dirTerms` key carries the list so the gap is inspectable; nothing \
                 upstream reads it yet."
                    .to_owned(),
            );
            for ns in affected {
                out.push(format!(
                    "  - `{}`: {}",
                    ns.mount,
                    ns.dir_terms
                        .iter()
                        .map(|t| format!("`{}{t}`", ns.iri_base))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    out
}

// -- shared path arithmetic -------------------------------------------------
//
// Every adapter needs the same answers about where a term lives, and they
// must agree, so the arithmetic is here rather than repeated five times.

/// Whether a term is served from a directory rather than a flat file.
pub fn is_dir(ns: &NamespaceEntry, local: &str) -> bool {
    ns.layout == "dir" || ns.dir_terms.iter().any(|d| d == local)
}

/// The site path a term's HTML is requested at, without the origin.
pub fn term_request_path(ns: &NamespaceEntry, local: &str) -> String {
    if is_dir(ns, local) {
        format!("{}{local}/", ns.mount)
    } else {
        format!("{}{local}", ns.mount)
    }
}

/// The site path of one representation of a term.
pub fn term_file_path(ns: &NamespaceEntry, local: &str, rep: &Representation) -> String {
    let Some(suffix) = rep.suffix.as_deref() else {
        return format!("{}{}", ns.mount, rep.namespace_file);
    };
    match (is_dir(ns, local), suffix.is_empty()) {
        (true, true) => format!("{}{local}/index.html", ns.mount),
        (true, false) => format!("{}{local}/index{suffix}", ns.mount),
        (false, true) => format!("{}{local}.html", ns.mount),
        (false, false) => format!("{}{local}{suffix}", ns.mount),
    }
}

/// The representations a term actually has, which is every one with a suffix.
pub fn term_representations(ns: &NamespaceEntry) -> Vec<&Representation> {
    ns.representations
        .iter()
        .filter(|r| r.suffix.is_some())
        .collect()
}

/// The extension aliases `?format=` accepts.
pub fn format_alias(media_type: &str) -> &'static str {
    match media_type {
        "text/html" => "html",
        "text/markdown" => "md",
        "text/turtle" => "ttl",
        "application/ld+json" => "jsonld",
        "application/rdf+xml" => "rdf",
        "application/n-triples" => "nt",
        _ => "",
    }
}

/// Whether a media type carries RDF, for the convention's RDF fallback rule.
pub fn is_rdf(media_type: &str) -> bool {
    matches!(
        media_type,
        "text/turtle"
            | "application/ld+json"
            | "application/rdf+xml"
            | "application/n-triples"
            | "application/trig"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namespace(layout: &str, dir_terms: &[&str]) -> NamespaceEntry {
        NamespaceEntry {
            id: "ex".to_owned(),
            kind: "ontology".to_owned(),
            iri_base: "https://example.org/vocab/".to_owned(),
            doc_base: "https://example.org/vocab/".to_owned(),
            mount: "/vocab/".to_owned(),
            resolver_prefix: None,
            layout: layout.to_owned(),
            dir_terms: dir_terms.iter().map(|s| (*s).to_owned()).collect(),
            resolver_type: "strict".to_owned(),
            prefix: None,
            title: None,
            version: None,
            version_iri: None,
            status: None,
            licence: None,
            default_type: "text/html".to_owned(),
            status_code: 303,
            cache_max_age: 86400,
            representations: vec![
                Representation {
                    media_type: "text/html".to_owned(),
                    suffix: Some(String::new()),
                    namespace_file: "vocab/index.html".to_owned(),
                },
                Representation {
                    media_type: "text/turtle".to_owned(),
                    suffix: Some(".ttl".to_owned()),
                    namespace_file: "vocab/ex.ttl".to_owned(),
                },
                Representation {
                    media_type: "application/rdf+xml".to_owned(),
                    suffix: None,
                    namespace_file: "vocab/ex.rdf".to_owned(),
                },
            ],
            terms: vec!["Widget".to_owned(), "index".to_owned()],
            reserved: Vec::new(),
            llms_txt: "https://example.org/vocab/llms.txt".to_owned(),
            versions: Vec::new(),
            cache_control: "public".to_owned(),
            snapshot_cache_control: "public, immutable".to_owned(),
        }
    }

    #[test]
    fn a_term_the_site_owns_the_name_of_is_routed_to_its_directory() {
        let ns = namespace("flat", &["index"]);
        assert_eq!(term_request_path(&ns, "Widget"), "/vocab/Widget");
        assert_eq!(term_request_path(&ns, "index"), "/vocab/index/");

        let html = &ns.representations[0];
        let ttl = &ns.representations[1];
        assert_eq!(term_file_path(&ns, "Widget", html), "/vocab/Widget.html");
        assert_eq!(term_file_path(&ns, "Widget", ttl), "/vocab/Widget.ttl");
        // Without this the adapter would send `/vocab/index` to the scheme's
        // own page, which is the collision the dir-term layout exists to
        // prevent.
        assert_eq!(
            term_file_path(&ns, "index", html),
            "/vocab/index/index.html"
        );
        assert_eq!(term_file_path(&ns, "index", ttl), "/vocab/index/index.ttl");
    }

    #[test]
    fn a_representation_with_no_suffix_is_not_offered_per_term() {
        let ns = namespace("flat", &[]);
        let types: Vec<&str> = term_representations(&ns)
            .iter()
            .map(|r| r.media_type.as_str())
            .collect();
        // RDF/XML exists only at the namespace level here, so a term must not
        // be advertised as having one.
        assert_eq!(types, vec!["text/html", "text/turtle"]);
    }

    #[test]
    fn the_host_names_round_trip() {
        for h in Host::ALL {
            assert_eq!(Host::parse(h.as_str()), Some(h));
        }
        assert_eq!(Host::parse("geocities"), None);
    }
}
