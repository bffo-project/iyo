//! Judging one reply against one resolved case. Pure: no network, no
//! filesystem.
//!
//! `resolve` decides what a compliant host should answer; `http::fetch` asks
//! a live one; this is the comparison in between, and the only place that
//! turns "here is what came back" into "this host does or does not
//! implement the negotiation rules".

use crate::conform::resolve::{Resolved, ResolvedExpect};
use crate::http;

/// The outcome of checking one resolved case against one reply.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Verdict {
    pub case: Resolved,
    pub passed: bool,
    /// A short description of what the origin actually sent, for the
    /// report. Not the reason a check failed -- that is `consequence`.
    pub got: String,
    /// What a failure means to a reader, not merely which byte differed.
    /// `None` exactly when `passed` is true; every failing path below sets
    /// it, so a verdict never fails silently.
    pub consequence: Option<String>,
}

/// Judge one reply against the case it was made for.
pub fn judge(case: &Resolved, reply: &http::Reply) -> Verdict {
    let got = describe(reply);

    // A host that could not be reached at all fails every expectation the
    // same way; nothing below this line has anything to compare.
    if reply.status == 0 {
        return fail(case, got, "the host could not be reached");
    }

    if let Some(consequence) = check_expectation(case, reply) {
        return fail(case, got, consequence);
    }

    // The convention's three normative response headers apply to every
    // negotiated response. A 404 is not a negotiated response, so `Absent` is
    // exempt -- these are not checked there even when the reply happens to
    // carry them, because a gate that starts expecting headers a 404 has no
    // obligation to send is a gate inventing its own requirements.
    if let Some(expected_cache_control) = case.expect.cache_control()
        && let Some(consequence) = check_normative_headers(reply, expected_cache_control)
    {
        return fail(case, got, consequence);
    }

    Verdict {
        case: case.clone(),
        passed: true,
        got,
        consequence: None,
    }
}

fn fail(case: &Resolved, got: String, consequence: impl Into<String>) -> Verdict {
    Verdict {
        case: case.clone(),
        passed: false,
        got,
        consequence: Some(consequence.into()),
    }
}

/// A short summary of what the origin sent, independent of whether it was
/// the right thing.
///
/// `reply.location` comes from `curl`'s own `%{redirect_url}`, which -- for
/// a server that sends a relative `Location` -- is resolved against the
/// *request* URL, not just copied from the response header. A credentialed
/// `--origin` therefore reappears inside it even though the server itself
/// never saw the credential, so it is redacted here rather than trusted as
/// server-controlled.
fn describe(reply: &http::Reply) -> String {
    if reply.status == 0 {
        return "no response".to_owned();
    }
    let mut got = reply.status.to_string();
    if !reply.content_type.is_empty() {
        got.push(' ');
        got.push_str(&reply.content_type);
    }
    if !reply.location.is_empty() {
        got.push_str(" -> ");
        got.push_str(&http::redact_url(&reply.location));
    }
    got
}

/// The expectation-specific check: does this reply satisfy what `resolve`
/// computed for this case? `None` means it does.
fn check_expectation(case: &Resolved, reply: &http::Reply) -> Option<String> {
    match &case.expect {
        ResolvedExpect::Absent => check_absent(reply),
        ResolvedExpect::Serve {
            media_type,
            body_contains,
            ..
        } => check_serve(reply, media_type, body_contains).or_else(|| check_link(case, reply)),
        ResolvedExpect::Redirect {
            location, status, ..
        } => check_redirect(reply, location, *status).or_else(|| check_link(case, reply)),
        ResolvedExpect::File { media_type, .. } => check_file(reply, media_type),
    }
}

/// The negative perimeter: a path the manifest does not mint must answer 404,
/// not merely "not 200". A 2xx and a 3xx are different failures with different
/// consequences for a reader, and the negative perimeter exists to tell them
/// apart from a real term.
fn check_absent(reply: &http::Reply) -> Option<String> {
    match reply.status {
        404 => None,
        200..=299 => Some(
            "the origin answers a page for an IRI the manifest does not mint; \
             a client cannot tell a real term from a typo"
                .to_owned(),
        ),
        300..=399 => Some(
            "an IRI the manifest does not mint redirects somewhere, so a typo resolves".to_owned(),
        ),
        other => Some(format!(
            "expected 404 for an IRI the manifest does not mint; the origin answered {other} instead"
        )),
    }
}

/// The gate: this IRI must answer here, with the declared type, and the body
/// must name the identity IRI -- the only thing that tells the term's real
/// page apart from an application catch-all that answers 200 for anything.
///
/// One consequence line used to cover all three ways this can fail, and
/// named only one of them (a catch-all answering 200 for everything). Point
/// the gate at a site where every IRI 404s and every failure said "a
/// catch-all answers 200", flatly contradicting its own `got 404` line. A
/// confidently wrong consequence is worse than none, so the three causes are
/// told apart here.
fn check_serve(reply: &http::Reply, media_type: &str, body_contains: &str) -> Option<String> {
    if reply.status != 200 {
        return Some(
            "this term's IRI does not resolve at all; a reader following a published \
             identifier gets nothing"
                .to_owned(),
        );
    }
    if !reply.content_type_is(media_type) {
        let got = if reply.content_type.is_empty() {
            "no Content-Type".to_owned()
        } else {
            reply.content_type.clone()
        };
        return Some(format!(
            "this term's IRI answers with {got}, not {media_type}; a client that asked for \
             one representation gets a different one back"
        ));
    }
    if !body_names_the_subject(reply.body.as_deref(), body_contains) {
        return Some(
            "the response is not this term's page; an application catch-all answers 200 for every unknown path"
                .to_owned(),
        );
    }
    None
}

/// Whether the body names `body_contains` as *this* response's own identity,
/// not merely as a substring of some other IRI that happens to start the
/// same way. `https://example.org/ontology/Format` is a substring of
/// `https://example.org/ontology/FormatVersion`, and BFFO publishes both
/// `Format` and `FormatVersion` in one namespace; `Role::Term` takes
/// `terms.first()`, so an unanchored `.contains` would let a host serving
/// `FormatVersion`'s page for `/ontology/Format` pass this check.
///
/// Every page this tool writes carries `<link rel="canonical" href="…">`
/// with the identity IRI, so the closing quote after `href="…` is the
/// delimiter that stops a longer IRI from matching.
///
/// A `Serve` expectation is always the namespace's default representation,
/// which `render` only ever mints as HTML: `resolve` refuses a `serve` case
/// asking for any other type rather than resolving one whose file and link
/// header would contradict the type it judges against. An earlier version
/// of this function carried a second branch for RDF subjects in angle
/// brackets or quotes; nothing could reach it, and an unreachable branch
/// reads as coverage that is not there.
fn body_names_the_subject(body: Option<&str>, iri: &str) -> bool {
    let Some(body) = body else { return false };
    body.contains(&format!("href=\"{iri}\""))
}

/// The gate: negotiation must land on the exact sibling the manifest
/// advertises, and with the status code the manifest's own configuration
/// chose (the convention recommends 303, but the gate holds a publisher to
/// what they configured, not to the recommendation).
///
/// `location` is the site-root-relative path `resolve` computed the sibling
/// at; the reply came from whatever origin is under test, which is not
/// necessarily the manifest's own, so it is rehosted onto `reply.url`'s
/// origin before comparing -- the same move `probe::rehost` makes going the
/// other direction.
fn check_redirect(reply: &http::Reply, location: &str, status: u16) -> Option<String> {
    let expected_location = format!("{}{location}", origin_of(&reply.url));
    if reply.location != expected_location {
        return Some(
            "negotiation redirects somewhere other than the sibling it advertises".to_owned(),
        );
    }
    if reply.status != status {
        return Some(format!(
            "negotiation used status {} where the manifest declares {status}",
            reply.status
        ));
    }
    None
}

/// The gate: a file asked for by name is not a negotiation; it only has to
/// answer as itself.
fn check_file(reply: &http::Reply, media_type: &str) -> Option<String> {
    if reply.status == 200 && reply.content_type_is(media_type) {
        return None;
    }
    Some(
        "the file does not resolve as itself; a sibling that names it points at nothing".to_owned(),
    )
}

/// The scheme and host of an absolute URL, e.g. `https://example.org` from
/// `https://example.org/vocab/Thing`. The same slice `adapter::apache`'s
/// `origin` and `probe`'s `rehost` compute; kept as its own copy here
/// because none of the three call each other and coupling three unrelated
/// modules over a five-line string slice costs more than duplicating it.
fn origin_of(url: &str) -> &str {
    let rest = url.split_once("//").map(|(_, r)| r).unwrap_or(url);
    match rest.find('/') {
        Some(i) => &url[..url.len() - (rest.len() - i)],
        None => url,
    }
}

/// Signposting (`describedby`, `cite-as`, and friends) is a
/// SHOULD, and a gate must not fail a publisher for declining a SHOULD. So
/// this is deliberately *not* "does the reply carry rel=cite-as" checked
/// against the specification -- it is a byte-for-byte compare against
/// `expected_link`, which is `Some` only when `resolve` computed one from
/// *this manifest's own configuration* (see `Resolved::expected_link`).
/// When a publisher's configuration produces no such relation,
/// `expected_link` is `None` and this function does nothing, which is
/// exactly what lets that publisher pass. Do not add a separate "has
/// rel=cite-as" assertion beside this one: that reintroduces the SHOULD
/// problem this comparison exists to avoid.
///
/// This is also the check that caught a real defect (commit 125bf97): a
/// release's `describedby` pointing at its parent's `llms.txt` instead of
/// its own. A check that only asked "is there a describedby relation" would
/// have called that healthy; only comparing where it pointed caught it.
fn check_link(case: &Resolved, reply: &http::Reply) -> Option<String> {
    let expected = case.expected_link.as_deref()?;
    if reply.link == expected {
        return None;
    }
    Some(
        "the Link header does not match what the manifest says this host emits; \
         a reader following a signposting relation such as describedby or cite-as \
         can land on the wrong resource with nothing to show it went wrong"
            .to_owned(),
    )
}

/// The convention's header rule: every negotiated response carries `Vary:
/// Accept`, an explicit `Cache-Control` equal to what the manifest declares
/// for this path, and open CORS. Not checked on `Absent`, which `judge` short-
/// circuits before calling this.
///
/// `Cache-Control` used to be checked only for presence, which is the gate's
/// blind spot: if `negotiate::snapshot_entry` stopped substituting
/// `snapshot_cache_control`, `iyo serve` would send the mutable `max-age=300`
/// policy inside an immutable release and this check would still pass,
/// because *some* Cache-Control was still there. Comparing against
/// `expected_cache_control` -- `entry.cache_control`, computed once in
/// `resolve::one` -- is the same move the gate already makes for `status_code`
/// and for signposting via `expected_link`: the manifest states which the
/// publisher chose, so the gate compares against the manifest.
fn check_normative_headers(reply: &http::Reply, expected_cache_control: &str) -> Option<String> {
    if !reply.varies_on_accept() {
        return Some(
            "the response carries no Vary: Accept; the convention requires it on every \
             negotiated response, so a cache can serve the wrong representation to \
             the next request"
                .to_owned(),
        );
    }
    if reply.cache_control != expected_cache_control {
        let got = if reply.cache_control.is_empty() {
            "(none)".to_owned()
        } else {
            reply.cache_control.clone()
        };
        return Some(format!(
            "the response's Cache-Control is {got}, where the manifest declares \
             {expected_cache_control}; a release meant to be cached forever would be \
             revalidated like the moving latest, or the reverse"
        ));
    }
    if reply.cors != "*" {
        return Some(
            "Access-Control-Allow-Origin is not \"*\"; the convention requires open CORS \
             on every negotiated response, so a browser script on another origin \
             cannot read it"
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conform::resolve::{Resolved, ResolvedExpect};

    /// The Cache-Control value most fixtures below agree on: `reply()`
    /// sends it by default, so an expectation built with it passes unless a
    /// test deliberately perturbs one side or the other.
    const CACHE_CONTROL: &str = "public, max-age=60";

    fn case(expect: ResolvedExpect) -> Resolved {
        Resolved {
            name: "c".to_owned(),
            role: crate::conform::cases::Role::AbsentName,
            group: "negative perimeter".to_owned(),
            namespace: "/vocab/".to_owned(),
            path: "/vocab/iyo-absent-name".to_owned(),
            accept: Some("text/turtle".to_owned()),
            query: None,
            expect,
            expected_link: None,
        }
    }

    fn reply(status: u16, content_type: &str, body: Option<&str>) -> crate::http::Reply {
        crate::http::Reply {
            url: "https://example.org/vocab/iyo-absent-name".to_owned(),
            accept: "text/turtle".to_owned(),
            status,
            content_type: content_type.to_owned(),
            vary: "Accept".to_owned(),
            link: String::new(),
            location: String::new(),
            cache_control: "public, max-age=60".to_owned(),
            cors: "*".to_owned(),
            body: body.map(str::to_owned),
        }
    }

    #[test]
    fn an_unminted_name_answering_a_page_fails_and_says_why() {
        // This is the failure the gate exists for: an application catch-all
        // answers 200 text/html for every unknown path, and every check that
        // stops at the status line calls it healthy.
        let v = judge(
            &case(ResolvedExpect::Absent),
            &reply(200, "text/html", None),
        );
        assert!(!v.passed);
        let consequence = v.consequence.expect("a consequence");
        assert!(consequence.contains("does not mint"), "{consequence}");
    }

    #[test]
    fn an_unminted_name_redirecting_fails_and_says_why() {
        let mut r = reply(302, "", None);
        r.location = "https://example.org/vocab/elsewhere".to_owned();
        let v = judge(&case(ResolvedExpect::Absent), &r);
        assert!(!v.passed);
        let consequence = v.consequence.expect("a consequence");
        assert!(consequence.contains("redirects somewhere"), "{consequence}");
    }

    #[test]
    fn an_unminted_name_answering_a_server_error_fails() {
        // Neither of the two named cases (2xx, 3xx), but still not the 404
        // the negative perimeter requires.
        let v = judge(&case(ResolvedExpect::Absent), &reply(500, "", None));
        assert!(!v.passed);
        assert!(v.consequence.unwrap().contains("expected 404"));
    }

    #[test]
    fn an_unminted_name_answering_404_passes() {
        assert!(judge(&case(ResolvedExpect::Absent), &reply(404, "", None)).passed);
    }

    #[test]
    fn a_page_whose_body_does_not_name_the_iri_fails() {
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/Thing".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/Thing.html".to_owned(),
        };
        let shell = reply(200, "text/html", Some("<html><body>app</body></html>"));
        assert!(!judge(&case(expect.clone()), &shell).passed);

        let real = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/vocab/Thing\">"),
        );
        assert!(judge(&case(expect), &real).passed);
    }

    /// IMPORTANT 5: `body_contains` used to be an unanchored substring, so
    /// `https://example.org/ontology/Format` -- a substring of
    /// `https://example.org/ontology/FormatVersion` -- would have matched a
    /// response that actually named the longer IRI. This is the BFFO shape:
    /// `Format` and `FormatVersion` published in one namespace, `Role::Term`
    /// taking `terms.first()`. A host serving `FormatVersion`'s page for
    /// `/ontology/Format` must still fail on the body check.
    #[test]
    fn a_body_naming_a_longer_iri_that_merely_starts_the_same_way_fails() {
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/ontology/Format".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "ontology/Format.html".to_owned(),
        };
        let wrong_term = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/ontology/FormatVersion\">"),
        );
        let v = judge(&case(expect.clone()), &wrong_term);
        assert!(!v.passed);
        assert!(
            v.consequence
                .unwrap()
                .contains("an application catch-all answers 200")
        );

        let right_term = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/ontology/Format\">"),
        );
        assert!(judge(&case(expect), &right_term).passed);
    }

    /// IMPORTANT 4: one consequence line used to cover all three ways a
    /// `serve` case can fail, and named only the least likely one (a
    /// catch-all answering 200). Pointed at a site where every IRI 404s,
    /// every failure said "a catch-all answers 200", flatly contradicting
    /// the case's own `got 404` line.
    #[test]
    fn a_serve_case_that_does_not_resolve_at_all_names_that_and_not_a_catchall() {
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/Thing".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/Thing.html".to_owned(),
        };
        let v = judge(&case(expect), &reply(404, "", None));
        assert!(!v.passed);
        let consequence = v.consequence.expect("a consequence");
        assert!(
            consequence.contains("does not resolve at all"),
            "{consequence}"
        );
        assert!(
            !consequence.contains("catch-all"),
            "a 404 must not be blamed on a catch-all: {consequence}"
        );
    }

    #[test]
    fn a_serve_case_with_the_wrong_content_type_names_the_mismatch() {
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/Thing".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/Thing.html".to_owned(),
        };
        let v = judge(&case(expect), &reply(200, "application/json", None));
        assert!(!v.passed);
        let consequence = v.consequence.expect("a consequence");
        assert!(consequence.contains("text/html"), "{consequence}");
        assert!(consequence.contains("application/json"), "{consequence}");
        assert!(
            !consequence.contains("catch-all"),
            "a type mismatch must not be blamed on a catch-all: {consequence}"
        );
    }

    #[test]
    fn a_normative_header_missing_fails_even_when_the_status_is_right() {
        // The convention's header rule: every negotiated response carries Vary,
        // an explicit Cache-Control, and open CORS.
        let mut r = reply(404, "", None);
        r.cors = String::new();
        let v = judge(&case(ResolvedExpect::Absent), &r);
        assert!(v.passed, "CORS is not required on a 404");

        let expect = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let mut r = reply(200, "text/turtle", None);
        r.cache_control = String::new();
        assert!(!judge(&case(expect), &r).passed);
    }

    /// CRITICAL 2: Cache-Control used to be checked only for presence, not
    /// against the manifest's declared value -- so a release serving the
    /// mutable `latest` policy in place of the immutable snapshot one would
    /// have passed, because *some* Cache-Control was still sent. A release
    /// case must carry the immutable value; a latest case must not.
    #[test]
    fn a_cache_control_present_but_different_from_the_manifest_fails() {
        const SNAPSHOT_CACHE: &str = "public, max-age=31536000, immutable";
        const LATEST_CACHE: &str = "public, max-age=300, must-revalidate";
        assert_ne!(SNAPSHOT_CACHE, LATEST_CACHE);

        let expect = ResolvedExpect::Redirect {
            location: "/vocab/0.1.0/Thing.ttl".to_owned(),
            status: 303,
            cache_control: SNAPSHOT_CACHE.to_owned(),
        };
        let mut r = reply(303, "text/html", None);
        r.location = "https://example.org/vocab/0.1.0/Thing.ttl".to_owned();

        // Sent the mutable "latest" policy where the release requires the
        // immutable one: exactly what a snapshot that forgot to substitute
        // `snapshot_cache_control` would do, and what a presence-only check
        // cannot see.
        r.cache_control = LATEST_CACHE.to_owned();
        let v = judge(
            &Resolved {
                expect: expect.clone(),
                ..case(ResolvedExpect::Absent)
            },
            &r,
        );
        assert!(!v.passed);
        let consequence = v.consequence.expect("a consequence");
        assert!(consequence.contains("Cache-Control"), "{consequence}");
        assert!(consequence.contains(LATEST_CACHE), "{consequence}");
        assert!(consequence.contains(SNAPSHOT_CACHE), "{consequence}");

        // The correct, immutable value passes.
        r.cache_control = SNAPSHOT_CACHE.to_owned();
        assert!(
            judge(
                &Resolved {
                    expect,
                    ..case(ResolvedExpect::Absent)
                },
                &r
            )
            .passed
        );
    }

    #[test]
    fn a_missing_vary_accept_fails() {
        let expect = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let mut r = reply(200, "text/turtle", None);
        r.vary = String::new();
        let v = judge(&case(expect), &r);
        assert!(!v.passed);
        assert!(v.consequence.unwrap().contains("Vary"));
    }

    #[test]
    fn a_vary_header_that_only_names_accept_encoding_fails_a_serve_case() {
        // "Accept-Encoding" contains the substring "accept"; a substring
        // check would wrongly call this compliant with the header rule, which
        // asks specifically whether the host varies on Accept.
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/iyo-absent-name".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/iyo-absent-name.html".to_owned(),
        };
        let mut r = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/vocab/iyo-absent-name\">"),
        );
        r.vary = "Accept-Encoding".to_owned();
        let v = judge(&case(expect), &r);
        assert!(!v.passed);
        assert!(v.consequence.unwrap().contains("Vary"));
    }

    #[test]
    fn a_cors_header_that_is_not_open_fails() {
        let expect = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let mut r = reply(200, "text/turtle", None);
        r.cors = "https://example.org".to_owned();
        let v = judge(&case(expect), &r);
        assert!(!v.passed);
        assert!(
            v.consequence
                .unwrap()
                .contains("Access-Control-Allow-Origin")
        );
    }

    #[test]
    fn a_file_answering_with_the_wrong_type_fails() {
        let expect = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let r = reply(200, "application/json", None);
        let v = judge(&case(expect), &r);
        assert!(!v.passed);
        assert!(
            v.consequence
                .unwrap()
                .contains("does not resolve as itself")
        );
    }

    // The brief's four fixed tests are `an_unminted_name_answering_404_passes`,
    // `a_page_whose_body_does_not_name_the_iri_fails`, and
    // `a_normative_header_missing_fails_even_when_the_status_is_right`,
    // above, plus `an_unminted_name_answering_a_page_fails_and_says_why` at
    // the top of this module. Everything else in this file, including the
    // three just above, covers a branch none of those four exercise: the
    // other two `Absent` outcomes, each normative header on its own, a
    // wrong-type `file`, `expected_link`, `redirect`, and an unreachable
    // host.

    #[test]
    fn a_link_header_that_does_not_match_the_manifest_fails_even_though_everything_else_is_right() {
        // The check that caught 125bf97: a release's describedby pointed at
        // its parent's llms.txt. Status, content type and body were all
        // correct; only where the Link header pointed was wrong, and a
        // check that only asked "is a describedby relation present" would
        // have missed it.
        let mut c = case(ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/iyo-absent-name".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/iyo-absent-name.html".to_owned(),
        });
        c.expected_link = Some("<llms.txt>; rel=\"describedby\"".to_owned());
        let mut r = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/vocab/iyo-absent-name\">"),
        );

        r.link = "<../llms.txt>; rel=\"describedby\"".to_owned();
        let v = judge(&c, &r);
        assert!(!v.passed);
        assert!(v.consequence.unwrap().contains("Link header"));

        r.link = c.expected_link.clone().unwrap();
        assert!(judge(&c, &r).passed);
    }

    #[test]
    fn a_serve_case_with_no_expected_link_does_not_check_one() {
        // `expected_link` is `None` for a manifest that declines the
        // signposting SHOULD. Nothing should fail just because the reply
        // itself carries no Link header either.
        let expect = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/iyo-absent-name".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/iyo-absent-name.html".to_owned(),
        };
        let r = reply(
            200,
            "text/html",
            Some("<link rel=\"canonical\" href=\"https://example.org/vocab/iyo-absent-name\">"),
        );
        assert!(judge(&case(expect), &r).passed);
    }

    #[test]
    fn a_redirect_to_the_advertised_sibling_passes_after_rehosting_onto_the_origin_under_test() {
        // `location` on a resolved redirect is site-root-relative
        // ("/vocab/Thing.ttl"); the manifest was not necessarily built for
        // the origin under test, so it is rehosted onto `reply.url`'s
        // origin before comparing.
        let c = Resolved {
            expect: ResolvedExpect::Redirect {
                location: "/vocab/Thing.ttl".to_owned(),
                status: 303,
                cache_control: CACHE_CONTROL.to_owned(),
            },
            ..case(ResolvedExpect::Absent)
        };
        let mut r = reply(303, "text/html", None);
        r.location = "https://example.org/vocab/Thing.ttl".to_owned();
        assert!(judge(&c, &r).passed);
    }

    #[test]
    fn a_redirect_to_a_different_place_fails() {
        let c = Resolved {
            expect: ResolvedExpect::Redirect {
                location: "/vocab/Thing.ttl".to_owned(),
                status: 303,
                cache_control: CACHE_CONTROL.to_owned(),
            },
            ..case(ResolvedExpect::Absent)
        };
        let mut r = reply(303, "text/html", None);
        r.location = "https://example.org/vocab/Other.ttl".to_owned();
        let v = judge(&c, &r);
        assert!(!v.passed);
        assert!(
            v.consequence
                .unwrap()
                .contains("somewhere other than the sibling")
        );
    }

    #[test]
    fn a_redirect_with_the_right_location_but_the_wrong_status_fails() {
        // The gate: the manifest's own `status_code` is the standard, not
        // the convention's recommended 303.
        let c = Resolved {
            expect: ResolvedExpect::Redirect {
                location: "/vocab/Thing.ttl".to_owned(),
                status: 303,
                cache_control: CACHE_CONTROL.to_owned(),
            },
            ..case(ResolvedExpect::Absent)
        };
        let mut r = reply(302, "text/html", None);
        r.location = "https://example.org/vocab/Thing.ttl".to_owned();
        assert!(!judge(&c, &r).passed);
    }

    #[test]
    fn a_file_answering_200_with_its_declared_type_passes() {
        let expect = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let r = reply(200, "text/turtle", None);
        assert!(judge(&case(expect), &r).passed);
    }

    #[test]
    fn a_charset_parameter_on_the_content_type_does_not_fail_a_real_host() {
        // `iyo serve` -- the reference implementation the contract calls fully
        // compliant -- sends `text/html; charset=utf-8`. A comparison that
        // demands the bare media type byte for byte would fail the one
        // harness this gate is supposed to pass against.
        let serve = ResolvedExpect::Serve {
            media_type: "text/html".to_owned(),
            body_contains: "https://example.org/vocab/iyo-absent-name".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
            file: "vocab/iyo-absent-name.html".to_owned(),
        };
        let r = reply(
            200,
            "text/html; charset=utf-8",
            Some("<link rel=\"canonical\" href=\"https://example.org/vocab/iyo-absent-name\">"),
        );
        assert!(judge(&case(serve), &r).passed);

        let file = ResolvedExpect::File {
            media_type: "text/turtle".to_owned(),
            cache_control: CACHE_CONTROL.to_owned(),
        };
        let r = reply(200, "text/turtle; charset=utf-8", None);
        assert!(judge(&case(file), &r).passed);
    }

    #[test]
    fn an_unreachable_host_fails_regardless_of_what_was_expected() {
        let v = judge(&case(ResolvedExpect::Absent), &reply(0, "", None));
        assert!(!v.passed);
        assert!(v.consequence.unwrap().contains("could not be reached"));
    }
}
