//! A development server that honours the manifest.
//!
//! Not a web server. It exists so that the negotiation contract can be
//! exercised before a deployment rather than after one, which matters because
//! the defect this whole tool exists to fix, every minted IRI answering 404,
//! is invisible from the built directory and obvious the moment something
//! serves it.
//!
//! Hand-rolled on `std::net`, with no dependency. A dev server for one
//! localhost origin needs a small subset of HTTP/1.1: a request line, headers
//! until a blank line, no body, no keep-alive, no chunked encoding. Taking a
//! web framework for that would be a larger commitment than the feature.
//! What it is not safe for is stated in the banner it prints.

use crate::negotiate::{self, Outcome};
use crate::render::manifest::{Manifest, NamespaceEntry};
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

/// How a request was answered, for the log line and for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// One request, already parsed.
#[derive(Debug, Clone)]
pub struct Request {
    pub path: String,
    pub query: Option<String>,
    pub accept: Option<String>,
}

/// Answer one request from a built site. Separated from the socket so that a
/// test can drive it without opening a port.
///
/// The static fallback below (`Outcome::PassThrough`) reads the filesystem
/// fresh on every request, through `read_exact`, rather than trusting a set
/// of paths captured once at startup: a dev server that only ever answers
/// from a startup snapshot needs a restart after every rebuild to see what
/// changed, which defeats the point of a dev server.
pub fn answer(root: &Utf8Path, manifest: &Manifest, request: &Request) -> Answer {
    let outcome = negotiate::resolve(
        manifest,
        &request.path,
        request.accept.as_deref(),
        request.query.as_deref(),
    );
    // The namespace this request falls under, whether or not the local name
    // is a term. `NamespaceEntry::cache_control` is documented as "what a
    // host should send for this namespace's own files" -- not only its
    // negotiated term pages -- so a namespace document requested at its own
    // mount, a sibling asked for by its exact name, and a nested namespace's
    // own root all carry it too. `target` already does the mount-matching
    // and release-segment substitution `resolve` uses, so reusing it here
    // cannot describe a release differently than the release describes
    // itself (commits 6913a01, 125bf97).
    let target = negotiate::target(manifest, &request.path);

    let mut headers = vec![
        ("Access-Control-Allow-Origin".to_owned(), "*".to_owned()),
        ("Vary".to_owned(), "Accept".to_owned()),
    ];

    // The Link header a negotiated response carries. Unlike Cache-Control
    // this is genuinely specific to negotiation (signposting is about
    // signposting a chosen representation among its siblings), so it stays
    // scoped to non-`PassThrough` outcomes.
    if !matches!(outcome, Outcome::PassThrough)
        && let Some((ns, local)) = &target
    {
        let chosen = negotiate::select(
            ns,
            request.accept.as_deref(),
            request
                .query
                .as_deref()
                .and_then(|q| {
                    q.split('&').find_map(|pair| {
                        let (key, value) = pair.split_once('=')?;
                        matches!(key, "format" | "_mediatype" | "_profile")
                            .then(|| value.to_owned())
                    })
                })
                .as_deref(),
        );
        headers.push((
            "Link".to_owned(),
            negotiate::link_header(manifest, ns, local, chosen),
        ));
    }

    match outcome {
        Outcome::Redirect { location, status } => {
            headers.push(("Location".to_owned(), location));
            push_cache_control(&mut headers, &target);
            Answer {
                status,
                headers,
                body: Vec::new(),
            }
        }
        Outcome::Serve { file, .. } => match read(root, &file) {
            Some(body) => {
                headers.push((
                    "Content-Type".to_owned(),
                    negotiate::media_type_of(&file).to_owned(),
                ));
                push_cache_control(&mut headers, &target);
                Answer {
                    status: 200,
                    headers,
                    body,
                }
            }
            // The manifest says this term resolves and the file is not there.
            // That is a build defect, and saying so is more useful than a
            // bare 404 that looks like a routing mistake.
            None => not_found(
                root,
                headers,
                format!(
                    "the manifest resolves this term to {file}, which this build did not write"
                ),
            ),
        },
        Outcome::PassThrough => {
            let candidates = [
                request.path.trim_start_matches('/').to_owned(),
                format!("{}index.html", request.path.trim_start_matches('/')),
                format!("{}/index.html", request.path.trim_start_matches('/')),
            ];
            for candidate in candidates {
                if candidate.is_empty() {
                    continue;
                }
                // Case-exact: a candidate must be a path the build actually
                // wrote, byte for byte, checked by re-reading its parent
                // directory on every request rather than trusting a set
                // captured once at startup -- `read_exact`'s own doc comment
                // says why. `read` alone is not enough: an OS free to fold
                // case (APFS does; ext4 does not) would resolve a
                // case-variant candidate to the real file and answer 200 to
                // a name nobody minted.
                if let Some(body) = read_exact(root, &candidate) {
                    headers.push((
                        "Content-Type".to_owned(),
                        negotiate::media_type_of(&candidate).to_owned(),
                    ));
                    push_cache_control(&mut headers, &target);
                    return Answer {
                        status: 200,
                        headers,
                        body,
                    };
                }
            }
            // A path with no owning namespace (or one that names no real
            // file within it) is a 404, which is exempt from the header rule:
            // a not-found page is not "this namespace's own files".
            not_found(root, headers, String::new())
        }
    }
}

/// The convention's header rule: an explicit Cache-Control on a response that
/// is not a 404. `target` is `None` for a path with no owning namespace, in
/// which case there is no manifest-declared policy to send. `snapshot_entry`
/// (inside `negotiate::target`) already substitutes `snapshot_cache_control`
/// into `cache_control` for anything inside a release, so reading this one
/// field is correct there too.
fn push_cache_control(
    headers: &mut Vec<(String, String)>,
    target: &Option<(NamespaceEntry, String)>,
) {
    if let Some((ns, _)) = target {
        headers.push(("Cache-Control".to_owned(), ns.cache_control.clone()));
    }
}

fn not_found(root: &Utf8Path, mut headers: Vec<(String, String)>, note: String) -> Answer {
    headers.push((
        "Content-Type".to_owned(),
        "text/html; charset=utf-8".to_owned(),
    ));
    let body = read(root, "404.html").unwrap_or_else(|| b"<h1>Not found</h1>".to_vec());
    if !note.is_empty() {
        headers.push(("X-Iyo-Note".to_owned(), note));
    }
    Answer {
        status: 404,
        headers,
        body,
    }
}

/// Read a file from the site, refusing anything that climbs out of it.
fn read(root: &Utf8Path, relative: &str) -> Option<Vec<u8>> {
    // A dev server is still a server: `..` in a path must not reach the
    // filesystem above the site.
    if relative.split('/').any(|s| s == "..") {
        return None;
    }
    let path: Utf8PathBuf = root.join(relative);
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

/// Read a file from the static fallback, case-exactly: every segment of
/// `relative` must appear in its own directory's listing byte for byte, not
/// merely open successfully. See `exists_exact` for why a whole-path open
/// is not enough.
///
/// An earlier fix built a set of every path the build wrote once at startup
/// and checked membership in that instead of asking the filesystem at all.
/// That reintroduced a different defect: a dev server answering from a
/// snapshot needs restarting after every rebuild to see what changed, which
/// is exactly the workflow this server exists to support. Re-reading the
/// path's own directories fresh on every request, rather than trusting a
/// walk of the whole tree taken once, keeps the case-exactness with none of
/// the staleness.
fn read_exact(root: &Utf8Path, relative: &str) -> Option<Vec<u8>> {
    if relative.split('/').any(|s| s == "..") {
        return None;
    }
    if !exists_exact(root, relative) {
        return None;
    }
    std::fs::read(root.join(relative)).ok()
}

/// Whether every path segment of `relative` names a directory entry under
/// `root`, compared byte for byte against each directory's own listing --
/// not merely whether the OS can open the resulting path.
///
/// Checking only the final segment against its immediate parent is not
/// enough: on a case-insensitive filesystem (APFS folds case by default),
/// *opening* `root/vocabulary/category/Index` to list it succeeds and
/// resolves to the real `index` directory underneath -- so a check that
/// only re-verified the last segment (`index.html`) would find it sitting
/// right there and answer 200 for a path nobody minted. Walking one segment
/// at a time and re-reading each directory from the last *verified* one,
/// rather than asking the filesystem to resolve the whole path first, is
/// what stops the OS's own case-folding from ever getting a vote.
fn exists_exact(root: &Utf8Path, relative: &str) -> bool {
    let mut dir = root.to_owned();
    for segment in relative.split('/') {
        let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
            return false;
        };
        let found = entries
            .flatten()
            .any(|entry| entry.file_name() == std::ffi::OsStr::new(segment));
        if !found {
            return false;
        }
        dir = dir.join(segment);
    }
    true
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        303 => "See Other",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    }
}

fn handle(root: &Utf8Path, manifest: &Manifest, mut stream: TcpStream, quiet: bool) -> Result<()> {
    let peer = stream.try_clone().context("cloning the connection")?;
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();

    let mut accept = None;
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("accept") {
                accept = Some(value.trim().to_owned());
            }
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    // Drain a body so the connection closes cleanly rather than resetting.
    if content_length > 0 {
        let mut sink = vec![0u8; content_length.min(1 << 20)];
        let _ = reader.read_exact(&mut sink);
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
        None => (target.clone(), None),
    };
    let path = percent_decode(&path);

    let request = Request {
        path,
        query,
        accept,
    };
    let mut result = answer(root, manifest, &request);
    // A HEAD is a GET without the body, which is what a link checker sends.
    if method.eq_ignore_ascii_case("HEAD") {
        result.body.clear();
    }

    if !quiet {
        let extra = result
            .header("Location")
            .map(|l| format!(" -> {l}"))
            .unwrap_or_default();
        eprintln!(
            "{method} {target} [{}] {} {}{extra}",
            request.accept.as_deref().unwrap_or("-"),
            result.status,
            reason(result.status)
        );
    }

    let mut out = Vec::new();
    let _ = write!(
        out,
        "HTTP/1.1 {} {}\r\n",
        result.status,
        reason(result.status)
    );
    for (name, value) in &result.headers {
        let _ = write!(out, "{name}: {value}\r\n");
    }
    let _ = write!(out, "Content-Length: {}\r\n", result.body.len());
    let _ = write!(out, "Connection: close\r\n\r\n");
    out.extend_from_slice(&result.body);
    stream.write_all(&out)?;
    stream.flush()?;
    Ok(())
}

fn percent_decode(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_owned())
}

/// Serve a built site until interrupted.
/// Serve `root` until interrupted.
///
/// `json` prints one object on stdout as soon as the socket is bound, then
/// nothing more. `--port 0` lets the OS pick, and before this the chosen
/// port existed only inside a sentence on stderr, so no script could find
/// out where the server had bound. The line is flushed immediately: a
/// caller is blocked reading it.
pub fn run(root: &Utf8Path, port: u16, quiet: bool, json: bool) -> Result<()> {
    let manifest_path = root.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!("reading {manifest_path}; run `iyo build` into this directory first")
    })?;
    let manifest: Manifest =
        serde_json::from_str(text.as_str()).with_context(|| format!("parsing {manifest_path}"))?;

    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    let bound = listener.local_addr()?.port();

    if json {
        let document = serde_json::json!({
            "schema_version": crate::model::SCHEMA_VERSION,
            "root": root.as_str(),
            "address": format!("127.0.0.1:{bound}"),
            "url": format!("http://127.0.0.1:{bound}/"),
            "port": bound,
            "namespaces": manifest.namespaces.len(),
            "terms": manifest.namespaces.iter().map(|n| n.terms.len()).sum::<usize>(),
        });
        let mut out = std::io::stdout();
        writeln!(out, "{}", serde_json::to_string(&document)?)?;
        out.flush()?;
    }
    eprintln!("serving {root} on http://127.0.0.1:{bound}/");
    eprintln!(
        "{} namespaces, {} terms; negotiation from manifest.json",
        manifest.namespaces.len(),
        manifest
            .namespaces
            .iter()
            .map(|n| n.terms.len())
            .sum::<usize>()
    );
    eprintln!("for development only: one connection at a time, no TLS, bound to localhost.\n");

    for stream in listener.incoming() {
        let stream = stream.context("accepting a connection")?;
        // One at a time: a preview server has one user, and a thread pool
        // would be more machinery than the job needs.
        if let Err(e) = handle(root, &manifest, stream, quiet) {
            eprintln!("error: {e}");
        }
    }
    Ok(())
}

/// Bind a port and answer requests on a background thread, for tests.
pub fn spawn(root: &Utf8Path, manifest: Manifest) -> Result<(u16, std::thread::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("binding an ephemeral port")?;
    let port = listener.local_addr()?.port();
    let root = root.to_owned();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let _ = handle_quietly(&root, &manifest, stream);
        }
    });
    Ok((port, handle))
}

fn handle_quietly(root: &Utf8Path, manifest: &Manifest, stream: TcpStream) -> Result<()> {
    handle(root, manifest, stream, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_cannot_climb_out_of_the_site() {
        let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"));
        // `Cargo.toml` is in the root, so this proves the file exists and
        // that the guard is what stops the traversal rather than the file
        // being absent.
        assert!(read(root, "Cargo.toml").is_some());
        assert!(read(root, "../Cargo.toml").is_none());
        assert!(read(root, "src/../Cargo.toml").is_none());
    }

    #[test]
    fn media_types_come_from_the_extension() {
        assert_eq!(
            negotiate::media_type_of("a/b.ttl"),
            "text/turtle; charset=utf-8"
        );
        assert_eq!(
            negotiate::media_type_of("a/b.jsonld"),
            "application/ld+json"
        );
        assert_eq!(negotiate::media_type_of("a/b"), "application/octet-stream");
    }

    /// Builds `testdata/mini` into a directory of its own and returns it with
    /// its manifest, exactly as `tests/probe.rs` does for the same fixture.
    fn build_mini(name: &str) -> (Utf8PathBuf, Manifest) {
        let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = crate::load::expand_inputs(&["testdata/mini".to_owned()], &root).unwrap();
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
        let output = crate::render::render(&ctx).unwrap();
        let manifest = crate::render::manifest::build(&ctx);

        let dir = Utf8PathBuf::from_path_buf(std::env::temp_dir())
            .unwrap()
            .join(format!("iyo-serve-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        output.write(&dir).unwrap();
        (dir, manifest)
    }

    #[test]
    fn a_negotiated_response_carries_the_manifests_cache_control() {
        let (dir, manifest) = build_mini("cache-control");
        let ns = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/" && !n.terms.is_empty() && !n.versions.is_empty())
            .expect("a namespace with terms and a release");
        let local = ns.terms.first().expect("a term");

        let request = Request {
            path: format!("{}{local}", ns.mount),
            query: None,
            accept: Some("text/html".to_owned()),
        };
        let reply = answer(&dir, &manifest, &request);
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(
            reply.header("Cache-Control"),
            Some(ns.cache_control.as_str()),
            "a term's own response must carry the manifest's cache-control"
        );

        let version = ns.versions.first().expect("a release");
        let release_request = Request {
            path: format!("{}{}/{local}", ns.mount, version.segment),
            query: None,
            accept: Some("text/html".to_owned()),
        };
        let release_reply = answer(&dir, &manifest, &release_request);
        assert_eq!(release_reply.status, 200, "{release_reply:?}");
        assert_eq!(
            release_reply.header("Cache-Control"),
            Some(ns.snapshot_cache_control.as_str()),
            "a term inside a release must carry the immutable snapshot cache-control, \
             not the parent's"
        );
        assert_ne!(
            ns.cache_control, ns.snapshot_cache_control,
            "the two assertions above are meaningless if these ever match"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_namespace_document_asked_for_by_name_also_carries_cache_control() {
        // `NamespaceEntry::cache_control` is documented as the policy for
        // "this namespace's own files", not only its negotiated term pages.
        // A namespace document requested at its own mount resolves as
        // `PassThrough` (there is no local name to negotiate), so this is
        // the case that would slip through a fix scoped to `Serve`/
        // `Redirect` outcomes only.
        let (dir, manifest) = build_mini("namespace-cache-control");
        let ns = manifest
            .namespaces
            .iter()
            .find(|n| n.mount == "/vocab/")
            .expect("the /vocab/ namespace");

        let request = Request {
            path: ns.mount.clone(),
            query: None,
            accept: Some("text/html".to_owned()),
        };
        let reply = answer(&dir, &manifest, &request);
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(
            reply.header("Cache-Control"),
            Some(ns.cache_control.as_str()),
            "a namespace document is still this namespace's own file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_case_variant_of_a_dir_term_does_not_resolve_through_the_filesystem() {
        // `/vocabulary/category/index/` is written for the term `index`
        // (dir-term layout). `Index` is not a term; a case-sensitive
        // identifier space must not answer it by falling through to a
        // case-insensitive filesystem lookup (APFS folds case by default).
        let (dir, manifest) = build_mini("case-exact");
        let request = Request {
            path: "/vocabulary/category/Index".to_owned(),
            query: None,
            accept: Some("text/html".to_owned()),
        };
        let reply = answer(&dir, &manifest, &request);
        assert_eq!(
            reply.status, 404,
            "a case-variant of a real term must not be served: {reply:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// IMPORTANT 7: the case-folding fix turned a dev server that read the
    /// filesystem per request into one that answers from a set built once at
    /// startup. `iyo build --out dist && iyo serve dist`, then rebuild in
    /// another shell: every newly-written file 404s until restart. A file
    /// written after the server started -- exactly what a rebuild between
    /// requests produces -- must still be served.
    #[test]
    fn a_file_written_after_the_server_started_is_served_without_a_restart() {
        let (dir, manifest) = build_mini("late-write");

        // Written after the site was built -- a rebuild landing between two
        // requests, exactly what this server must not need a restart for.
        std::fs::write(dir.join("late.html"), b"<html>late</html>").unwrap();

        let request = Request {
            path: "/late.html".to_owned(),
            query: None,
            accept: None,
        };
        let reply = answer(&dir, &manifest, &request);
        assert_eq!(
            reply.status, 200,
            "a file written after the server started must be served, not just after a restart: {reply:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
