//! Content negotiation, as `docs/output-convention.md` defines it under
//! "Content negotiation — the rules".
//!
//! This is the second implementation of that section. The first is the
//! JavaScript the Cloudflare adapter generates, and it has to be JavaScript
//! because it runs in a Worker. Two implementations of one specification is
//! usually a liability; here it is turned into a check, because
//! `tests/hosts/` runs the same matrix of `Accept` headers through both and
//! any disagreement is a failure. A specification with one implementation is
//! only as precise as that implementation happens to be.
//!
//! The rules that are easy to get wrong, and are therefore stated rather than
//! implied:
//!
//! - `Accept` is parsed with q-values. Matching it with a substring test is
//!   how `text/turtle;q=0.1, text/html` comes to serve Turtle to a browser.
//! - An explicit media type beats a wildcard whatever the q-values say, so a
//!   browser sending `*/*` gets the default and not whichever type happens to
//!   be declared first.
//! - A client that asked only for RDF types this release does not have still
//!   gets RDF. It plainly wanted data.
//! - Nothing answers 406. A usable answer beats a correct refusal.

use crate::render::manifest::{Manifest, NamespaceEntry, Representation};

/// What a request resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Serve this file from the output tree, with `Vary: Accept`.
    Serve {
        /// Path relative to the output root, with no leading slash.
        file: String,
        media_type: String,
    },
    /// Redirect to a representation-specific URL, which is the cache-safe
    /// choice on a CDN that ignores `Vary`.
    Redirect { location: String, status: u16 },
    /// Not a term of any namespace: let the file layer answer, and 404 if it
    /// cannot.
    PassThrough,
}

/// One parsed `Accept` entry.
#[derive(Debug, Clone)]
struct Accepted {
    media_type: String,
    q: f32,
}

/// Parse `Accept` per RFC 9110: most preferred first, and stable within a
/// q-value so the client's own order survives.
pub fn parse_accept(header: &str) -> Vec<String> {
    let mut parsed: Vec<(usize, Accepted)> = header
        .split(',')
        .enumerate()
        .filter_map(|(index, part)| {
            let mut pieces = part.trim().split(';');
            let media_type = pieces.next()?.trim().to_ascii_lowercase();
            if media_type.is_empty() {
                return None;
            }
            let mut q = 1.0f32;
            for piece in pieces {
                let Some((key, value)) = piece.split_once('=') else {
                    continue;
                };
                if key.trim() == "q"
                    && let Ok(parsed) = value.trim().parse::<f32>()
                {
                    q = parsed;
                }
            }
            if q <= 0.0 {
                return None;
            }
            Some((index, Accepted { media_type, q }))
        })
        .collect();
    parsed.sort_by(|(ai, a), (bi, b)| {
        b.q.partial_cmp(&a.q)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ai.cmp(bi))
    });
    parsed.into_iter().map(|(_, a)| a.media_type).collect()
}

fn is_wildcard(media_type: &str) -> bool {
    media_type == "*/*" || media_type.ends_with("/*")
}

fn looks_like_rdf(media_type: &str) -> bool {
    ["rdf", "turtle", "ld+json", "n-triples", "n3", "trig"]
        .iter()
        .any(|m| media_type.contains(m))
}

/// The alias table `?format=` accepts.
pub fn alias(value: &str) -> Option<&str> {
    Some(match value {
        "html" => "text/html",
        "md" | "markdown" => "text/markdown",
        "ttl" | "turtle" => "text/turtle",
        "jsonld" | "json" => "application/ld+json",
        "rdf" | "xml" => "application/rdf+xml",
        "nt" => "application/n-triples",
        // Already a media type: ConnegP's `_mediatype` passes one directly.
        other if other.contains('/') => other,
        _ => return None,
    })
}

/// Choose a representation for one request.
pub fn select<'a>(
    ns: &'a NamespaceEntry,
    accept: Option<&str>,
    override_type: Option<&str>,
) -> &'a Representation {
    let reps: Vec<&Representation> = crate::adapter::term_representations(ns);
    let default = reps
        .iter()
        .find(|r| r.media_type == ns.default_type)
        .copied()
        .or_else(|| reps.first().copied())
        .expect("a namespace always publishes at least one representation");

    if let Some(wanted) = override_type.and_then(alias)
        && let Some(hit) = reps.iter().find(|r| r.media_type == wanted)
    {
        return hit;
    }

    let parsed = parse_accept(accept.unwrap_or(""));

    // Explicit types first, in the client's order of preference.
    for media_type in parsed.iter().filter(|m| !is_wildcard(m)) {
        if let Some(hit) = reps.iter().find(|r| r.media_type == *media_type) {
            return hit;
        }
    }

    // A wildcard means "anything", and the default is what we consider best.
    if let Some(wildcard) = parsed.iter().find(|m| is_wildcard(m)) {
        if wildcard == "text/*" {
            let text = reps
                .iter()
                .find(|r| r.media_type.starts_with("text/") && r.media_type == ns.default_type)
                .or_else(|| reps.iter().find(|r| r.media_type.starts_with("text/")));
            if let Some(hit) = text {
                return hit;
            }
        }
        return default;
    }

    // Asked for data we do not have: answer with data, not with a web page.
    if !parsed.is_empty()
        && parsed.iter().any(|m| looks_like_rdf(m))
        && let Some(rdf) = reps.iter().find(|r| looks_like_rdf(&r.media_type))
    {
        return rdf;
    }

    default
}

/// The namespace a request belongs to and the local name within it.
///
/// Every caller needs the same pair, and a caller that works it out for itself
/// gets the parent namespace for a request inside a release: right about the
/// paths by luck, wrong about anything the release owns.
pub fn target(manifest: &Manifest, path: &str) -> Option<(NamespaceEntry, String)> {
    // The longest matching mount wins: `/vocabulary/category/` before
    // `/vocabulary/`, or every scheme resolves as a term of its parent.
    let ns = manifest
        .namespaces
        .iter()
        .filter(|n| path.starts_with(&n.mount))
        .max_by_key(|n| n.mount.len())?;

    let local = path[ns.mount.len()..].trim_end_matches('/');
    // A release is published under a segment of its own, and its terms
    // negotiate exactly as the newest ones do. Moving the namespace onto that
    // segment is what the renderer does to write the snapshot in the first
    // place, so a resolver and the files on disk cannot disagree about it.
    if let Some((segment, rest)) = local.split_once('/')
        && let Some(version) = ns.versions.iter().find(|v| v.segment == segment)
    {
        return Some((snapshot_entry(ns, version), rest.to_owned()));
    }
    Some((ns.clone(), local.to_owned()))
}

/// Resolve one request path against the manifest.
pub fn resolve(
    manifest: &Manifest,
    path: &str,
    accept: Option<&str>,
    query: Option<&str>,
) -> Outcome {
    let Some((ns, local)) = target(manifest, path) else {
        return Outcome::PassThrough;
    };
    resolve_in(&ns, &local, accept, query)
}

/// Resolve a local name that has already been matched to its namespace.
fn resolve_in(
    ns: &NamespaceEntry,
    local: &str,
    accept: Option<&str>,
    query: Option<&str>,
) -> Outcome {
    // Anything with a separator still left is a nested document, and anything
    // naming an extension is being asked for by name.
    if local.is_empty() || local.contains('/') {
        return Outcome::PassThrough;
    }
    if crate::adapter::term_representations(ns).iter().any(|r| {
        r.suffix
            .as_deref()
            .is_some_and(|s| !s.is_empty() && local.ends_with(s))
    }) {
        return Outcome::PassThrough;
    }
    if ns.resolver_type == "strict" && !ns.terms.iter().any(|t| t == local) {
        return Outcome::PassThrough;
    }

    let override_type = query.and_then(|q| {
        q.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            matches!(key, "format" | "_mediatype" | "_profile").then(|| value.to_owned())
        })
    });
    let chosen = select(ns, accept, override_type.as_deref());

    if chosen.media_type == ns.default_type {
        Outcome::Serve {
            file: crate::adapter::term_file_path(ns, local, chosen)
                .trim_start_matches('/')
                .to_owned(),
            media_type: chosen.media_type.clone(),
        }
    } else {
        Outcome::Redirect {
            location: public_url(ns, local, chosen),
            status: ns.status_code,
        }
    }
}

/// The namespace as it exists inside one release: the same terms, the same
/// layout and the same representations, mounted under the version segment.
///
/// A release is what you cite when you cite a release, so its documents are
/// their own identity rather than the moving one, and they never expire.
pub fn snapshot_entry(ns: &NamespaceEntry, version: &crate::version::Snapshot) -> NamespaceEntry {
    let mount = format!("{}{}/", ns.mount, version.segment);
    let dir = mount.trim_start_matches('/').to_owned();
    NamespaceEntry {
        iri_base: version.url.clone(),
        doc_base: version.url.clone(),
        representations: ns
            .representations
            .iter()
            .map(|r| Representation {
                namespace_file: format!(
                    "{dir}{}",
                    r.namespace_file
                        .rsplit_once('/')
                        .map_or(r.namespace_file.as_str(), |(_, f)| f)
                ),
                ..r.clone()
            })
            .collect(),
        mount,
        // A release writes its own agent index, as it writes its own PDF, so
        // that it stays readable after the newest one has moved on. Pointing
        // `describedby` at the parent's would describe the release by a file
        // that no longer lists the same terms.
        llms_txt: format!(
            "{}{}",
            version.url,
            ns.llms_txt
                .rsplit_once('/')
                .map_or(ns.llms_txt.as_str(), |(_, f)| f)
        ),
        // Nothing nests under a release, and nothing published after one
        // belongs to it.
        reserved: Vec::new(),
        versions: Vec::new(),
        cache_control: ns.snapshot_cache_control.clone(),
        ..ns.clone()
    }
}

/// The URL a representation is published at, without the origin.
pub fn public_url(ns: &NamespaceEntry, local: &str, rep: &Representation) -> String {
    let suffix = rep.suffix.as_deref().unwrap_or_default();
    match (crate::adapter::is_dir(ns, local), suffix.is_empty()) {
        (true, true) => format!("{}{local}/", ns.mount),
        (true, false) => format!("{}{local}/index{suffix}", ns.mount),
        (false, true) => format!("{}{local}", ns.mount),
        (false, false) => format!("{}{local}{suffix}", ns.mount),
    }
}

/// The `Link` header a negotiated response carries (FAIR Signposting).
pub fn link_header(
    manifest: &Manifest,
    ns: &NamespaceEntry,
    local: &str,
    chosen: &Representation,
) -> String {
    let reps = crate::adapter::term_representations(ns);
    let default = reps
        .iter()
        .find(|r| r.media_type == ns.default_type)
        .copied()
        .or_else(|| reps.first().copied());
    let origin = manifest.site_root.trim_end_matches('/');
    // `canonical` and `cite-as` name the resource and are absolute wherever
    // they are read. `alternate` and `describedby` name something the client
    // is expected to fetch, so they are relative references, which RFC 8288
    // resolves against the request URI: the same response is then correct from
    // the origin the manifest names, from a preview, and from a staging host.
    let from = crate::adapter::term_request_path(ns, local);
    let mut parts = Vec::new();
    if let Some(d) = default {
        parts.push(format!(
            "<{origin}{}>; rel=\"canonical\"",
            public_url(ns, local, d)
        ));
    }
    parts.push(format!("<{}{local}>; rel=\"cite-as\"", ns.iri_base));
    for rep in &reps {
        if rep.media_type == chosen.media_type {
            continue;
        }
        parts.push(format!(
            "<{}>; rel=\"alternate\"; type=\"{}\"",
            relative_to(&from, &public_url(ns, local, rep)),
            rep.media_type
        ));
    }
    parts.push(format!(
        "<{}>; rel=\"describedby\"; type=\"text/plain\"",
        relative_to(
            &from,
            ns.llms_txt.strip_prefix(origin).unwrap_or(&ns.llms_txt)
        )
    ));
    parts.join(", ")
}

/// A relative reference from one absolute site path to another, as RFC 3986
/// resolves it against the request URI.
pub fn relative_to(from: &str, to: &str) -> String {
    fn split(path: &str) -> (Vec<&str>, &str) {
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
        (dir.split('/').filter(|s| !s.is_empty()).collect(), file)
    }
    let (from_dirs, _) = split(from);
    let (to_dirs, to_file) = split(to);
    let shared = from_dirs
        .iter()
        .zip(to_dirs.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = "../".repeat(from_dirs.len() - shared);
    for dir in &to_dirs[shared..] {
        out.push_str(dir);
        out.push('/');
    }
    out.push_str(to_file);
    if out.is_empty() { "./".to_owned() } else { out }
}

/// The media type to send for a file, by extension.
pub fn media_type_of(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e) {
        Some("html") => "text/html; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("ttl") => "text/turtle; charset=utf-8",
        Some("jsonld") => "application/ld+json",
        Some("json") => "application/json",
        Some("nt") => "application/n-triples",
        Some("rdf" | "xml") => "application/rdf+xml; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("toml") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout exceptions a namespace carries are the reason the manifest
    /// exists, and a release republishes the same terms, so it must
    /// carry them too. The bundled fixture cannot express this, because the
    /// namespace with a reserved stem is not the one with a version IRI.
    #[test]
    fn a_release_keeps_the_layout_exceptions_of_the_terms_it_republishes() {
        let rep = |media_type: &str, suffix: &str, file: &str| Representation {
            media_type: media_type.to_owned(),
            suffix: Some(suffix.to_owned()),
            namespace_file: file.to_owned(),
        };
        let ns = NamespaceEntry {
            id: "ex".to_owned(),
            kind: "ontology".to_owned(),
            iri_base: "https://example.org/vocab/".to_owned(),
            doc_base: "https://example.org/vocab/".to_owned(),
            mount: "/vocab/".to_owned(),
            resolver_prefix: None,
            layout: "flat".to_owned(),
            dir_terms: vec!["index".to_owned()],
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
                rep("text/html", "", "vocab/index.html"),
                rep("text/turtle", ".ttl", "vocab/ex.ttl"),
            ],
            terms: vec!["index".to_owned(), "Widget".to_owned()],
            reserved: vec!["shapes".to_owned()],
            llms_txt: "https://example.org/vocab/llms.txt".to_owned(),
            versions: vec![crate::version::Snapshot {
                namespace: "https://example.org/vocab/".to_owned(),
                segment: "0.1.0".to_owned(),
                source: crate::version::Source::VersionInfo,
                url: "https://example.org/vocab/0.1.0/".to_owned(),
                version_iri: Some("https://example.org/vocab/0.1.0/".to_owned()),
                version_iri_resolves: true,
            }],
            cache_control: "public, max-age=86400".to_owned(),
            snapshot_cache_control: "public, max-age=31536000, immutable".to_owned(),
        };
        let snapshot = snapshot_entry(&ns, &ns.versions[0]);
        assert_eq!(snapshot.mount, "/vocab/0.1.0/");
        assert_eq!(snapshot.dir_terms, vec!["index".to_owned()]);
        // A release is cited as a release, and never changes again.
        assert_eq!(snapshot.iri_base, "https://example.org/vocab/0.1.0/");
        assert_eq!(snapshot.cache_control, ns.snapshot_cache_control);
        // Nothing published after a release belongs to it.
        assert!(snapshot.versions.is_empty());
        // And the exception still applies to the file the resolver picks.
        let turtle = snapshot
            .representations
            .iter()
            .find(|r| r.media_type == "text/turtle")
            .expect("the fixture publishes turtle");
        assert_eq!(
            public_url(&snapshot, "index", turtle),
            "/vocab/0.1.0/index/index.ttl"
        );
    }

    #[test]
    fn a_relative_reference_resolves_back_to_the_path_it_came_from() {
        // A sibling representation, which is the common case.
        assert_eq!(
            relative_to("/ontology/Format", "/ontology/Format.ttl"),
            "Format.ttl"
        );
        // A term served from a directory reaches its own siblings by name,
        // and the namespace's files by going up. Getting this backwards is
        // the layout exception breaking signposting instead of links.
        assert_eq!(
            relative_to("/ontology/index/", "/ontology/index/index.ttl"),
            "index.ttl"
        );
        assert_eq!(
            relative_to("/ontology/index/", "/ontology/llms.txt"),
            "../llms.txt"
        );
        // The namespace's own files from a flat term.
        assert_eq!(
            relative_to("/ontology/Format", "/ontology/llms.txt"),
            "llms.txt"
        );
        // Across namespaces, and into a release.
        assert_eq!(
            relative_to("/vocabulary/category/alignment", "/ontology/Format"),
            "../../ontology/Format"
        );
        assert_eq!(
            relative_to(
                "/ontology/0.1.0-draft/Format",
                "/ontology/0.1.0-draft/Format.md"
            ),
            "Format.md"
        );

        // Whatever it returns has to resolve back to the target, or the
        // header is confidently wrong. Check that rather than the spelling.
        fn resolve(base: &str, reference: &str) -> String {
            let dir = base.rsplit_once('/').map_or("", |(d, _)| d);
            let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
            for segment in reference.split('/') {
                match segment {
                    "" | "." => {}
                    ".." => {
                        segments.pop();
                    }
                    s => segments.push(s),
                }
            }
            let trailing = if reference.ends_with('/') { "/" } else { "" };
            format!("/{}{trailing}", segments.join("/"))
        }
        for (from, to) in [
            ("/ontology/Format", "/ontology/Format.ttl"),
            ("/ontology/index/", "/ontology/index/index.ttl"),
            ("/ontology/index/", "/ontology/llms.txt"),
            ("/vocabulary/category/alignment", "/ontology/Format"),
            (
                "/ontology/0.1.0-draft/Format",
                "/ontology/0.1.0-draft/Format.md",
            ),
        ] {
            assert_eq!(resolve(from, &relative_to(from, to)), to, "from {from}");
        }
    }

    #[test]
    fn accept_is_ordered_by_q_and_then_by_the_clients_own_order() {
        assert_eq!(
            parse_accept("text/turtle;q=0.1, text/html"),
            vec!["text/html", "text/turtle"]
        );
        assert_eq!(
            parse_accept("text/html;q=0.1, text/turtle"),
            vec!["text/turtle", "text/html"]
        );
        // Equal q keeps the order the client wrote.
        assert_eq!(
            parse_accept("text/markdown, text/turtle"),
            vec!["text/markdown", "text/turtle"]
        );
        // q=0 means "not this".
        assert_eq!(
            parse_accept("text/html;q=0, text/turtle"),
            vec!["text/turtle"]
        );
        assert!(parse_accept("").is_empty());
    }

    #[test]
    fn a_browsers_header_is_read_as_a_browser_means_it() {
        // Chrome's header. The wildcard must not win over text/html.
        let order = parse_accept("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8");
        assert_eq!(order.first().map(String::as_str), Some("text/html"));
        assert_eq!(order.last().map(String::as_str), Some("*/*"));
    }
}
