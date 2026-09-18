//! Check a live origin against the manifest.
//!
//! Everything else in this tool reasons about a directory of files. This is
//! the only part that finds out what a server actually does, which is a
//! different question and the one a user of a vocabulary experiences. The
//! measured starting point for BFFO is that all 171 minted IRIs answer 404
//! while every file needed to serve them exists in the repository: a defect
//! no amount of looking at the build could reveal.
//!
//! What is checked is what a consumer depends on, and each check names the
//! FOOPS! probe it stands for so a result can be compared with a public
//! conformance score.

use crate::adapter;
use crate::http;
use crate::negotiate;
use crate::render::manifest::Manifest;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
}

/// What a planned request is *about*, as the plan itself knows it.
///
/// `foops` decides VER2 from this rather than from the prose in a finding's
/// `message`. The string it used to match on, `"release "`, was a prefix
/// `plan` happened to write into `Ask::context` and `judge` happened to
/// interpolate into every message: two coincidences, either of which a
/// reworded message would have broken silently, turning VER2 into a pass
/// earned by a failed release document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// A namespace's own document, or one of its terms.
    Namespace,
    /// The agent index (`llms.txt`) beside a namespace document.
    AgentIndex,
    /// A versioned snapshot's document: the request FOOPS! VER2 is about.
    Release,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub level: Level,
    /// The FOOPS! probe this stands for, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foops: Option<&'static str>,
    pub url: String,
    pub accept: String,
    pub message: String,
    /// What the request that produced this finding was about. Internal
    /// bookkeeping for `foops`, not serialised: the JSON report's shape is
    /// a public interface and this adds nothing a consumer asked for.
    #[serde(skip)]
    pub subject: Subject,
}

/// One response, as `curl` reported it. An alias, not a redefinition, so
/// `probe`'s public surface — and the shape of `--full`'s JSON report —
/// is unchanged by `fetch` and its two extra headers moving to `http`.
pub type Response = crate::http::Reply;

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The version of this document's shape, first field so a consumer can
    /// branch on it before anything else (`docs/cli.md`, "`--json`").
    pub schema_version: &'static str,
    /// `origin` with any userinfo password masked (`user:***@host`): this
    /// is what gets sent to the wire, not what gets reported.
    pub origin: String,
    /// How many requests the plan called for, whether or not the deadline
    /// let every one of them run.
    pub requests: usize,
    /// How many requests actually ran before the deadline (or the plan's
    /// end, whichever came first). Equal to `requests` unless `truncated`.
    pub made: usize,
    /// Whether the deadline was reached before every planned request ran.
    /// `findings`, `errors` and `warnings` below reflect only the requests
    /// that did run; nothing here claims the rest would have passed.
    pub truncated: bool,
    pub findings: Vec<Finding>,
    pub errors: usize,
    pub warnings: usize,
    /// FOOPS! probe outcomes, so a run can be compared with a published
    /// conformance score rather than only with itself. `Some(false)` is a
    /// finding that contradicts the probe and survives truncation;
    /// `Some(true)` is a pass earned by evidence that relevant requests were
    /// both planned and made, and none of them failed; `None` covers two
    /// different reasons no such evidence exists -- the plan never had
    /// anything of that kind to ask (`not applicable`), or it did but the
    /// deadline cut the run short before any of those requests ran (`not
    /// measured`) -- kept indistinguishable here only because this field's
    /// shape predates the distinction. See `foops_status` for which one.
    pub foops: BTreeMap<&'static str, Option<bool>>,
    /// The same outcomes as `foops`, spelled out over all four states
    /// (`"pass"`, `"fail"`, `"not measured"`, `"not applicable"`) for a
    /// consumer that wants to tell "nothing to test" apart from "ran out of
    /// time". Additive: `foops` alone still serialises exactly as before.
    pub foops_status: BTreeMap<&'static str, &'static str>,
    /// Every response, when `--full` asked for them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub responses: Vec<Response>,
}

/// A request the probe intends to make.
#[derive(Debug, Clone)]
struct Ask {
    url: String,
    accept: String,
    /// What the caller expects, so the check reads at the call site.
    expect: Expect,
    /// What this request is about, for `foops`. Structural, unlike
    /// `context`, which exists to be read by a person.
    subject: Subject,
    context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expect {
    /// A page: 200 and an HTML content type.
    Page,
    /// A negotiated RDF or Markdown request: a redirect to a sibling, or the
    /// sibling served directly. Both are conforming; the convention prefers
    /// the redirect because it is cache-safe on a CDN that ignores `Vary`.
    Sibling {
        media_type: String,
        /// Where the redirect should land. A host that redirects somewhere
        /// else is as broken as one that does not redirect at all, and only
        /// comparing the target catches it.
        location: String,
    },
    /// A file asked for by name.
    File {
        media_type: String,
        /// Whether its absence breaks a consumer. A sibling a page advertises
        /// does; a convenience file does not.
        required: bool,
    },
}

/// Move a URL from the origin the manifest was built for onto the origin
/// being probed, so a staging deployment can be checked with the same file.
fn rehost(url: &str, from: &str, to: &str) -> String {
    match url.strip_prefix(from.trim_end_matches('/')) {
        Some(rest) => format!("{}{rest}", to.trim_end_matches('/')),
        None => url.to_owned(),
    }
}

/// Build the request list from the manifest.
fn plan(manifest: &Manifest, origin: &str, sample: Option<usize>) -> Vec<Ask> {
    let root = manifest.site_root.clone();
    let mut asks = Vec::new();

    for ns in &manifest.namespaces {
        let document = rehost(
            &format!("{}{}", root, ns.mount.trim_start_matches('/')),
            &root,
            origin,
        );
        asks.push(Ask {
            url: document.clone(),
            accept: "text/html".to_owned(),
            expect: Expect::Page,
            subject: Subject::Namespace,
            context: format!("the document of {}", ns.iri_base),
        });
        asks.push(Ask {
            url: rehost(&ns.llms_txt, &root, origin),
            accept: "text/plain".to_owned(),
            expect: Expect::File {
                media_type: "text/plain".to_owned(),
                required: false,
            },
            subject: Subject::AgentIndex,
            context: format!("the agent index of {}", ns.iri_base),
        });
        for version in &ns.versions {
            asks.push(Ask {
                url: rehost(&version.url, &root, origin),
                accept: "text/html".to_owned(),
                expect: Expect::Page,
                subject: Subject::Release,
                context: format!("release {} of {}", version.segment, ns.iri_base),
            });
        }

        // A release republishes the same terms at addresses of its own. A
        // probe that stops at the release document reports a healthy site
        // while every term inside the release answers 404, which is exactly
        // what the deployment did.
        for ns in std::iter::once(ns.clone())
            .chain(ns.versions.iter().map(|v| negotiate::snapshot_entry(ns, v)))
            .collect::<Vec<_>>()
            .iter()
        {
            term_asks(ns, origin, sample, &mut asks);
        }
    }
    asks
}

/// Every request that proves one namespace's terms resolve and negotiate.
fn term_asks(
    ns: &crate::render::manifest::NamespaceEntry,
    origin: &str,
    sample: Option<usize>,
    asks: &mut Vec<Ask>,
) {
    {
        let reps = adapter::term_representations(ns);
        let terms: Vec<&String> = match sample {
            Some(n) => ns.terms.iter().take(n).collect(),
            None => ns.terms.iter().collect(),
        };
        for local in terms {
            for rep in &reps {
                let url = format!(
                    "{}{}",
                    origin.trim_end_matches('/'),
                    negotiate::public_url(ns, local, rep)
                );
                // The IRI is requested at its own address, not at the
                // sibling's: negotiation is what is under test.
                let request = if rep.media_type == ns.default_type {
                    url.clone()
                } else {
                    format!(
                        "{}{}",
                        origin.trim_end_matches('/'),
                        adapter::term_request_path(ns, local)
                    )
                };
                if rep.media_type == ns.default_type {
                    asks.push(Ask {
                        url: request,
                        accept: rep.media_type.clone(),
                        expect: Expect::Page,
                        subject: Subject::Namespace,
                        context: format!("{}{}", ns.iri_base, local),
                    });
                    continue;
                }
                asks.push(Ask {
                    url: request,
                    accept: rep.media_type.clone(),
                    expect: Expect::Sibling {
                        media_type: rep.media_type.clone(),
                        location: url.clone(),
                    },
                    subject: Subject::Namespace,
                    context: format!("{}{}", ns.iri_base, local),
                });
                // And the sibling itself. A redirect that lands on a 404 is
                // a working negotiation to a missing file, which reads as
                // success in every check that stops at the redirect.
                asks.push(Ask {
                    url,
                    accept: rep.media_type.clone(),
                    expect: Expect::File {
                        media_type: rep.media_type.clone(),
                        required: true,
                    },
                    subject: Subject::Namespace,
                    context: format!("the {} of {}{}", rep.media_type, ns.iri_base, local),
                });
            }
        }
    }
}

/// Check one response against what was asked for.
fn judge(ask: &Ask, response: &Response, findings: &mut Vec<Finding>) {
    let add = |findings: &mut Vec<Finding>,
               rule: &'static str,
               level: Level,
               foops: Option<&'static str>,
               message: String| {
        findings.push(Finding {
            rule,
            level,
            foops,
            url: http::redact_url(&ask.url),
            accept: ask.accept.clone(),
            message,
            subject: ask.subject,
        });
    };

    if response.status == 0 {
        add(
            findings,
            "probe.unreachable",
            Level::Error,
            Some("URI1"),
            format!("{} could not be reached", ask.context),
        );
        return;
    }

    match &ask.expect {
        Expect::Page => {
            if response.status != 200 {
                add(
                    findings,
                    "probe.no-page",
                    Level::Error,
                    Some("URI1"),
                    format!("{} answered {}", ask.context, response.status),
                );
                return;
            }
            if !response.content_type_is("text/html") {
                add(
                    findings,
                    "probe.wrong-type",
                    Level::Error,
                    Some("URI1"),
                    format!(
                        "{} answered {:?} for a page request",
                        ask.context, response.content_type
                    ),
                );
            }
        }
        Expect::Sibling {
            media_type,
            location,
        } => {
            match response.status {
                // The convention's recommendation: a redirect to a
                // representation-specific URL, which is cache-safe where
                // `Vary` is ignored.
                301..=308 => {
                    if response.location.is_empty() {
                        add(
                            findings,
                            "probe.redirect-without-location",
                            Level::Error,
                            Some("CN1"),
                            format!("{} redirected with no Location", ask.context),
                        );
                    } else if response.location.trim_end_matches('/')
                        != location.trim_end_matches('/')
                    {
                        add(
                            findings,
                            "probe.wrong-redirect",
                            Level::Error,
                            Some("CN1"),
                            format!(
                                "{} redirected to {} and not to {}",
                                ask.context,
                                http::redact_url(&response.location),
                                http::redact_url(location)
                            ),
                        );
                    }
                }
                200 => {
                    if !response.content_type_is(media_type) {
                        add(
                            findings,
                            "probe.not-negotiated",
                            Level::Error,
                            Some("CN1"),
                            format!(
                                "{} asked for {media_type} and got {:?}",
                                ask.context, response.content_type
                            ),
                        );
                    }
                }
                other => {
                    add(
                        findings,
                        "probe.no-representation",
                        Level::Error,
                        Some(if media_type.contains("markdown") {
                            "CN1"
                        } else {
                            "RDF1"
                        }),
                        format!("{} answered {other} for {media_type}", ask.context),
                    );
                    return;
                }
            }
            // A negotiated answer without `Vary: Accept` is a cache poisoning
            // waiting to happen: the next client gets whatever this one did.
            if !response.varies_on_accept() {
                add(
                    findings,
                    "probe.no-vary",
                    Level::Warning,
                    Some("CN1"),
                    format!("{} negotiated without Vary: Accept", ask.context),
                );
            }
            for relation in ["cite-as", "describedby"] {
                if !response.link.contains(relation) {
                    add(
                        findings,
                        "probe.no-signpost",
                        Level::Warning,
                        None,
                        format!("{} has no rel=\"{relation}\" link", ask.context),
                    );
                }
            }
        }
        Expect::File {
            media_type,
            required,
        } => {
            if response.status != 200 {
                add(
                    findings,
                    if *required {
                        "probe.no-representation"
                    } else {
                        "probe.missing-file"
                    },
                    if *required {
                        Level::Error
                    } else {
                        Level::Warning
                    },
                    required.then_some("RDF1"),
                    format!("{} answered {}", ask.context, response.status),
                );
            } else if !media_type.is_empty()
                && !response.content_type_is(media_type)
                && !response.content_type.is_empty()
            {
                add(
                    findings,
                    "probe.wrong-type",
                    if *required {
                        Level::Error
                    } else {
                        Level::Warning
                    },
                    required.then_some("RDF1"),
                    format!(
                        "{} is served as {:?}, not {media_type}",
                        ask.context, response.content_type
                    ),
                );
            }
        }
    }
}

/// Probe a live origin.
///
/// A header naming the origin and how many requests are planned goes to
/// stderr before the first request is made (clig.dev G22: something within
/// 100ms), and on a TTY only, a progress line updates as batches complete.
/// `deadline` bounds the whole run's wall-clock time; once it is reached the
/// run stops and returns what it gathered rather than continuing towards a
/// host that may never answer.
pub fn run(
    manifest: &Manifest,
    origin: &str,
    sample: Option<usize>,
    timeout: u32,
    keep_responses: bool,
    deadline: Duration,
) -> Result<Report> {
    if !http::available() {
        return Err(crate::Failure::err(
            crate::exit::ENVIRONMENT,
            "curl was not found on PATH",
            "`iyo probe` shells out to curl to make requests; install curl, or use \
             `iyo conform --cases FILE` which plans without making any",
        ));
    }
    let asks = plan(manifest, origin, sample);
    eprintln!(
        "probe: {} request{} planned for {}",
        asks.len(),
        if asks.len() == 1 { "" } else { "s" },
        http::redact_url(origin)
    );

    let started = Instant::now();
    let show_progress = http::stderr_is_terminal();
    let mut findings = Vec::new();
    let mut responses = Vec::new();
    // One entry per executed request, in plan order: did it produce an
    // error-level finding? `foops` decides every probe from this.
    let mut failed: Vec<bool> = Vec::new();
    let mut made = 0usize;
    let mut truncated = false;

    for batch in asks.chunks(http::BATCH) {
        // An interrupt is the same shape of gap as the deadline: the
        // requests past this point never ran, so the run speaks only for
        // the ones that did, and `truncated` is what says so. The terminal
        // sends SIGINT to the whole foreground process group, so the `curl`
        // child has already died rather than holding the run open for its
        // timeout (clig.dev G23).
        if crate::interrupt::pending() {
            truncated = true;
            break;
        }
        let elapsed = started.elapsed();
        if elapsed >= deadline {
            truncated = true;
            break;
        }
        let requests: Vec<http::Request> = batch
            .iter()
            .map(|ask| http::Request {
                url: ask.url.clone(),
                accept: Some(ask.accept.clone()),
            })
            .collect();
        // A batch already under way is what `--deadline`'s help text says
        // it stops, not just the next one: clamp this batch's per-request
        // timeout to what is left, so its slowest chunk cannot outrun the
        // deadline the check above just passed.
        let batch_timeout = http::clamped_timeout(deadline - elapsed, batch.len(), timeout);
        let answers = http::fetch(&requests, batch_timeout, false)?;
        for (ask, response) in batch.iter().zip(answers.iter()) {
            let before = findings.len();
            judge(ask, response, &mut findings);
            failed.push(findings[before..].iter().any(|f| f.level == Level::Error));
        }
        made += batch.len();
        if show_progress {
            eprint!("\rprobe: {made}/{} requests", asks.len());
            let _ = std::io::stderr().flush();
        }
        if keep_responses {
            // `url` is the request URL, echoed back by `parse`; `location`
            // is `curl`'s own `%{redirect_url}`, which for a relative
            // `Location` header is resolved against that same request URL
            // and can reintroduce its credential even though the response
            // header never carried one. Both are redacted before they leave
            // this function; every other field is a raw response header,
            // never derived from the request.
            responses.extend(answers.into_iter().map(|mut r| {
                r.url = http::redact_url(&r.url);
                r.location = http::redact_url(&r.location);
                r
            }));
        }
    }
    if show_progress {
        eprintln!();
    }

    // The verdicts are decided on every finding, before the collapse below
    // removes the evidence they are computed from.
    let outcomes = foops(&asks, made, &failed);
    collapse_unreachable(&mut findings, made, origin);
    let foops: BTreeMap<&'static str, Option<bool>> = outcomes
        .iter()
        .map(|(&probe, &outcome)| (probe, outcome.as_bool()))
        .collect();
    let foops_status: BTreeMap<&'static str, &'static str> = outcomes
        .iter()
        .map(|(&probe, &outcome)| (probe, outcome.as_str()))
        .collect();

    findings.sort_by(|a, b| (a.level, a.rule, &a.url).cmp(&(b.level, b.rule, &b.url)));
    Ok(Report {
        schema_version: crate::model::SCHEMA_VERSION,
        origin: http::redact_url(origin),
        requests: asks.len(),
        made,
        truncated,
        errors: findings.iter().filter(|f| f.level == Level::Error).count(),
        warnings: findings
            .iter()
            .filter(|f| f.level == Level::Warning)
            .count(),
        foops,
        foops_status,
        findings,
        responses,
    })
}

/// The four states a FOOPS! probe can report, kept distinct here so "there
/// was nothing to test" and "the deadline cut the run short" -- which
/// `foops`'s `bool`-shaped field must still collapse together, see its doc
/// comment -- are never confused while they are still easy to tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Fail,
    Pass,
    NotMeasured,
    NotApplicable,
}

impl Outcome {
    fn as_bool(self) -> Option<bool> {
        match self {
            Outcome::Fail => Some(false),
            Outcome::Pass => Some(true),
            Outcome::NotMeasured | Outcome::NotApplicable => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Outcome::Fail => "fail",
            Outcome::Pass => "pass",
            Outcome::NotMeasured => "not measured",
            Outcome::NotApplicable => "not applicable",
        }
    }
}

/// A request kind one FOOPS! probe cares about, so `foops` below can ask
/// "was anything of this kind planned" and "did any of it run" without
/// guessing from the findings alone -- the bug this replaces.
fn is_page(ask: &Ask) -> bool {
    matches!(ask.expect, Expect::Page)
}

fn is_sibling(ask: &Ask) -> bool {
    matches!(ask.expect, Expect::Sibling { .. })
}

/// The direct fetch of a required non-default representation -- the file a
/// negotiated request should land on, not the negotiation itself. A missing
/// or wrong-typed one is what `judge` reports as `RDF1` (`required.then_some
/// ("RDF1")` in the `Expect::File` arm), so this is that same request kind.
fn is_rdf_file(ask: &Ask) -> bool {
    matches!(ask.expect, Expect::File { required: true, .. })
}

/// A release document's own page request, which is what FOOPS! VER2 asks
/// about: does the version IRI resolve?
fn is_release(ask: &Ask) -> bool {
    matches!(ask.expect, Expect::Page) && ask.subject == Subject::Release
}

/// FOOPS! probe outcomes, expressed over the plan and which of its requests
/// failed, so that none of the three can disagree about the same evidence.
///
/// `failed[i]` says whether the i-th executed request produced an
/// error-level finding. Deciding from that rather than from a list of rule
/// ids per probe is what stops a probe passing on a request that never
/// happened: against a host refusing every connection, every request
/// produced `probe.unreachable`, which appeared in URI1's rule list and in
/// no other, so CN1 and RDF1 reported `pass` for a site where nothing
/// answered at all.
///
/// A finding is a `fail` that no truncation, and no absence of a plan,
/// can undo -- it was actually observed. Short of that, a probe needs
/// relevant requests to have been planned at all (else `not applicable`:
/// there was nothing to test, such as VER2 with no version IRI) and, having
/// been planned, to have actually run before the deadline (else `not
/// measured`). Only when relevant requests were both planned and made, and
/// none of them failed, is a probe a `pass` -- the same "reports success on
/// nothing" shape `conform`'s `conformant()` refuses (src/conform/mod.rs),
/// now closed for "nothing was ever asked" as well as "nothing came back in
/// time".
fn foops(asks: &[Ask], made: usize, failed: &[bool]) -> BTreeMap<&'static str, Outcome> {
    let executed = &asks[..made.min(asks.len())];

    let decide = |relevant: fn(&Ask) -> bool| -> Outcome {
        if executed
            .iter()
            .zip(failed)
            .any(|(ask, &failed)| failed && relevant(ask))
        {
            Outcome::Fail
        } else if !asks.iter().any(relevant) {
            Outcome::NotApplicable
        } else if !executed.iter().any(relevant) {
            Outcome::NotMeasured
        } else {
            Outcome::Pass
        }
    };

    let mut foops = BTreeMap::new();
    foops.insert("URI1", decide(is_page));
    foops.insert("CN1", decide(is_sibling));
    foops.insert("RDF1", decide(is_rdf_file));
    foops.insert("VER2", decide(is_release));
    foops
}

/// One unreachable host is one problem, not one problem per IRI.
///
/// A host that refuses connections produced a `probe.unreachable` finding
/// for every request in the plan: 119 on a ten-term fixture, and 56,003 on a
/// 16,000-term site. The count then scaled with the ontology rather than
/// with what was wrong, and the one fact worth reporting was buried under
/// tens of thousands of identical lines. When every request that ran failed
/// that way, they are replaced by the single finding they add up to.
///
/// Only when *every* one failed: a host that is up but has some IRIs
/// missing still reports them one by one, which is the case where the
/// per-IRI detail is the whole point.
fn collapse_unreachable(findings: &mut Vec<Finding>, made: usize, origin: &str) {
    let unreachable = findings
        .iter()
        .filter(|f| f.rule == "probe.unreachable")
        .count();
    if made < 2 || unreachable < made {
        return;
    }
    let accept = findings
        .first()
        .map(|f| f.accept.clone())
        .unwrap_or_default();
    findings.retain(|f| f.rule != "probe.unreachable");
    findings.insert(
        0,
        Finding {
            rule: "probe.host-unreachable",
            level: Level::Error,
            foops: Some("URI1"),
            url: http::redact_url(origin),
            accept,
            message: format!(
                "not one of the {made} requests reached this origin, so nothing about \
                 the site behind it could be checked"
            ),
            subject: Subject::Namespace,
        },
    );
}

/// How many requests a run would make, without making any. A caller sizing a
/// probe against a live site should be able to ask before it starts.
pub fn planned(manifest: &Manifest, origin: &str, sample: Option<usize>) -> usize {
    plan(manifest, origin, sample).len()
}

/// Fetch a manifest from a live origin, so a deployment can be checked
/// without the build that made it.
pub fn fetch_manifest(origin: &str, timeout: u32) -> Result<Manifest> {
    if !http::available() {
        // Reached from `probe` and from `conform`, so this names neither.
        return Err(crate::Failure::err(
            crate::exit::ENVIRONMENT,
            "curl was not found on PATH",
            "reading a manifest from a URL needs curl; install it, or point this \
             command at a built directory instead of an origin",
        ));
    }
    let url = format!("{}/manifest.json", origin.trim_end_matches('/'));
    let output = Command::new("curl")
        .args(["-sSL", "--max-time", &timeout.to_string(), &url])
        .output()
        .context("running curl")?;
    if !output.status.success() {
        bail!("could not fetch {}", http::redact_url(&url));
    }
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "{} is not a manifest this version understands",
            http::redact_url(&url)
        )
    })
}

/// The report, for a person.
pub fn summary(report: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "{} requests to {}\n", report.requests, report.origin);

    if report.truncated {
        let skipped = report.requests.saturating_sub(report.made);
        let _ = writeln!(
            s,
            "stopped early: the deadline was reached after {}/{} requests \
             ({skipped} skipped)\n",
            report.made, report.requests
        );
    }

    if report.findings.is_empty() {
        // With even one request skipped by the deadline, "every IRI
        // resolved" is not a claim this run can make: it only speaks for
        // the requests that ran.
        if !report.truncated {
            let _ = writeln!(s, "Every IRI resolved, in every representation.\n");
        }
    } else {
        // One line per distinct problem, with a count, because 644 copies of
        // the same 404 is one fact and not 644.
        let mut grouped: BTreeMap<(&str, Level), (usize, String)> = BTreeMap::new();
        for f in &report.findings {
            let entry = grouped
                .entry((f.rule, f.level))
                .or_insert((0, f.message.clone()));
            entry.0 += 1;
        }
        for ((rule, level), (count, example)) in grouped {
            let _ = writeln!(
                s,
                "{:<8} {count:>5}  {rule}\n           for example: {example}",
                format!("{level:?}").to_lowercase()
            );
        }
        s.push('\n');
    }

    let _ = writeln!(s, "FOOPS! probes");
    for (probe, status) in &report.foops_status {
        let label = if *status == "fail" { "FAIL" } else { status };
        let _ = writeln!(s, "  {probe:<6} {label}");
    }
    let _ = writeln!(
        s,
        "\n{} errors, {} warnings",
        report.errors, report.warnings
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `Ask` for exercising `foops` directly, without going
    /// through a real manifest and `plan`.
    fn ask(expect: Expect, context: &str) -> Ask {
        subject_ask(expect, Subject::Namespace, context)
    }

    /// A release document's ask. `foops` decides VER2 from `Subject`, so a
    /// test that means "a release was planned" has to say so structurally;
    /// writing "release 1.0.0 of ..." into `context` no longer makes it one.
    fn release_ask(context: &str) -> Ask {
        subject_ask(Expect::Page, Subject::Release, context)
    }

    fn subject_ask(expect: Expect, subject: Subject, context: &str) -> Ask {
        Ask {
            url: "http://127.0.0.1:9/x".to_owned(),
            accept: "text/html".to_owned(),
            expect,
            subject,
            context: context.to_owned(),
        }
    }

    fn sibling_ask(context: &str) -> Ask {
        ask(
            Expect::Sibling {
                media_type: "text/turtle".to_owned(),
                location: "http://127.0.0.1:9/vocab/Thing.ttl".to_owned(),
            },
            context,
        )
    }

    fn rdf_file_ask(context: &str) -> Ask {
        ask(
            Expect::File {
                media_type: "text/turtle".to_owned(),
                required: true,
            },
            context,
        )
    }

    #[test]
    fn a_url_moves_between_origins() {
        assert_eq!(
            rehost(
                "https://bffo.org/ontology/Format",
                "https://bffo.org/",
                "http://127.0.0.1:8788"
            ),
            "http://127.0.0.1:8788/ontology/Format"
        );
        // A URL that is not on the manifest's origin is left alone: a link to
        // another site is not ours to rewrite.
        assert_eq!(
            rehost(
                "https://example.com/x",
                "https://bffo.org/",
                "http://127.0.0.1:8788"
            ),
            "https://example.com/x"
        );
    }

    /// `iyo probe dist --origin … --deadline 0` makes zero requests, finds
    /// nothing (there is nothing to find anything in), and must not claim
    /// success: a truncated run only speaks for the requests that ran.
    #[test]
    fn a_truncated_report_with_no_findings_does_not_claim_success() {
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "http://127.0.0.1:9".to_owned(),
            requests: 12,
            made: 0,
            truncated: true,
            findings: Vec::new(),
            errors: 0,
            warnings: 0,
            foops: BTreeMap::new(),
            foops_status: BTreeMap::new(),
            responses: Vec::new(),
        };
        let text = summary(&report);
        assert!(
            !text.contains("Every IRI resolved"),
            "a truncated run with skipped requests claimed every IRI resolved: {text}"
        );
        assert!(
            text.contains("12 skipped") || text.contains("0/12"),
            "the report does not say how many requests were skipped: {text}"
        );
    }

    /// A run that finished within its deadline, with nothing to report,
    /// still gets the plain "every IRI resolved" line -- only a truncated
    /// run loses it.
    #[test]
    fn an_untruncated_report_with_no_findings_still_reports_success() {
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "http://127.0.0.1:9".to_owned(),
            requests: 12,
            made: 12,
            truncated: false,
            findings: Vec::new(),
            errors: 0,
            warnings: 0,
            foops: BTreeMap::new(),
            foops_status: BTreeMap::new(),
            responses: Vec::new(),
        };
        let text = summary(&report);
        assert!(
            text.contains("Every IRI resolved"),
            "a complete run with no findings should still report success: {text}"
        );
    }

    /// A plan with relevant requests for all four probes, none of them made
    /// before the deadline, must not report any probe as passing: `pass` is
    /// a claim earned by evidence, and truncation means none was gathered.
    /// A release ask is included so VER2 goes through the same
    /// evidence-based path as the other three, rather than a vacuous
    /// "nothing to check" exit.
    #[test]
    fn a_truncated_run_with_no_findings_reports_every_probe_as_not_measured() {
        let asks = vec![
            ask(Expect::Page, "the document of https://example.org/vocab#"),
            sibling_ask("https://example.org/vocab#Thing"),
            rdf_file_ask("the text/turtle of https://example.org/vocab#Thing"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        let result = foops(&asks, 0, &[]);
        for (probe, outcome) in &result {
            assert_eq!(
                *outcome,
                Outcome::NotMeasured,
                "{probe} should be not-measured, not pass, with requests planned but none made: {result:?}"
            );
        }
    }

    /// A finding observed before the deadline was reached is real evidence;
    /// truncation must not blanket-overwrite it back into "not measured".
    /// The other three probes, whose relevant requests were planned but
    /// never made, correctly have no evidence and so are not measured --
    /// this is the case a naive "if truncated { None }" applied after the
    /// fact, rather than per probe, would get wrong by clobbering the
    /// observed fail too.
    #[test]
    fn a_finding_survives_truncation_as_a_fail_not_a_non_measurement() {
        let asks = vec![
            // Only this first ask ran before the deadline; it is the one
            // that produced the finding below.
            ask(Expect::Page, "the document of https://example.org/vocab#"),
            sibling_ask("https://example.org/vocab#Thing"),
            rdf_file_ask("the text/turtle of https://example.org/vocab#Thing"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        // The first request ran and failed; the other three never ran.
        let result = foops(&asks, 1, &[true]);
        assert_eq!(
            result.get("URI1"),
            Some(&Outcome::Fail),
            "an observed failure must survive truncation: {result:?}"
        );
        assert_eq!(result.get("CN1"), Some(&Outcome::NotMeasured));
        assert_eq!(result.get("RDF1"), Some(&Outcome::NotMeasured));
        assert_eq!(result.get("VER2"), Some(&Outcome::NotMeasured));
    }

    /// VER2 is decided by which request failed, not by how a message
    /// reads. A release document that could not be reached is a VER2 fail
    /// whatever the text says, which is what the old
    /// `message.contains("release ")` check depended on.
    #[test]
    fn ver2_fails_on_a_failing_release_whatever_the_message_says() {
        let asks = vec![
            ask(Expect::Page, "the document of https://example.org/vocab#"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        // The namespace document answered; the release did not.
        let result = foops(&asks, asks.len(), &[false, true]);
        assert_eq!(result.get("VER2"), Some(&Outcome::Fail), "{result:?}");
        assert_eq!(result.get("URI1"), Some(&Outcome::Fail), "{result:?}");
    }

    /// And the converse: a term page failing does not fail VER2, however
    /// its message reads. Under the old check a finding whose prose
    /// mentioned a release failed VER2 while every release request answered
    /// cleanly.
    #[test]
    fn ver2_ignores_a_finding_that_only_mentions_a_release() {
        let asks = vec![
            ask(Expect::Page, "https://example.org/vocab#Thing"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        let result = foops(&asks, asks.len(), &[true, false]);
        assert_eq!(result.get("VER2"), Some(&Outcome::Pass), "{result:?}");
        assert_eq!(result.get("URI1"), Some(&Outcome::Fail), "{result:?}");
    }

    /// A host that refuses every connection fails every probe whose
    /// requests ran. Under the rule-list version, `probe.unreachable`
    /// appeared in URI1's list and in no other, so CN1 and RDF1 reported
    /// `pass` against a site where nothing answered at all.
    #[test]
    fn an_unreachable_host_fails_every_probe_that_was_asked() {
        let asks = vec![
            ask(Expect::Page, "the document of https://example.org/vocab#"),
            sibling_ask("https://example.org/vocab#Thing"),
            rdf_file_ask("the text/turtle of https://example.org/vocab#Thing"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        let result = foops(&asks, asks.len(), &[true, true, true, true]);
        for (probe, outcome) in &result {
            assert_eq!(*outcome, Outcome::Fail, "{probe} should fail: {result:?}");
        }
    }

    /// A plan with nothing of a probe's kind in it -- no version IRI for
    /// VER2 -- reports `not applicable`, not a pass earned by nothing. This
    /// is the bug this branch exists to close: VER2's old hardcoded
    /// `Some(true)` on the `!versioned` branch.
    #[test]
    fn ver2_is_not_applicable_when_no_release_was_ever_planned() {
        let asks = vec![ask(
            Expect::Page,
            "the document of https://example.org/vocab#",
        )];
        let result = foops(&asks, asks.len(), &vec![false; asks.len()]);
        assert_eq!(result.get("VER2"), Some(&Outcome::NotApplicable));
        // The one ask that was planned ran clean, so it is a real pass.
        assert_eq!(result.get("URI1"), Some(&Outcome::Pass));
    }

    /// The same "nothing to test" shape applies to the other three: a plan
    /// with no page request in it at all reports URI1 `not applicable`, for
    /// example a namespace with no terms and so no term pages to ask about.
    #[test]
    fn uri1_is_not_applicable_when_no_page_was_ever_planned() {
        let asks = vec![sibling_ask("https://example.org/vocab#Thing")];
        let result = foops(&asks, asks.len(), &vec![false; asks.len()]);
        assert_eq!(result.get("URI1"), Some(&Outcome::NotApplicable));
    }

    /// A complete, untruncated run with no findings is unaffected by this
    /// change: every probe still reports `Some(true)`, which serialises to
    /// the bare `true` `--json` has always printed, with no `null` and no
    /// new nesting.
    #[test]
    fn an_untruncated_result_serialises_exactly_as_before() {
        let asks = vec![
            ask(Expect::Page, "the document of https://example.org/vocab#"),
            sibling_ask("https://example.org/vocab#Thing"),
            rdf_file_ask("the text/turtle of https://example.org/vocab#Thing"),
            release_ask("release 1.0.0 of https://example.org/vocab#"),
        ];
        let result = foops(&asks, asks.len(), &vec![false; asks.len()]);
        let bools: BTreeMap<&'static str, Option<bool>> = result
            .iter()
            .map(|(&probe, &outcome)| (probe, outcome.as_bool()))
            .collect();
        let json = serde_json::to_value(&bools).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"CN1": true, "RDF1": true, "URI1": true, "VER2": true})
        );
    }

    /// The human summary spells out `not measured` and `not applicable`
    /// instead of silently calling either a pass, and the probe-name
    /// column stays aligned to the same width regardless of how long the
    /// outcome word is.
    #[test]
    fn the_summary_spells_out_every_non_pass_state_for_a_truncated_probe() {
        let mut foops_status = BTreeMap::new();
        foops_status.insert("CN1", "not measured");
        foops_status.insert("RDF1", "not applicable");
        foops_status.insert("URI1", "fail");
        foops_status.insert("VER2", "pass");
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "http://127.0.0.1:9".to_owned(),
            requests: 12,
            made: 0,
            truncated: true,
            findings: Vec::new(),
            errors: 0,
            warnings: 0,
            foops: BTreeMap::new(),
            foops_status,
            responses: Vec::new(),
        };
        let text = summary(&report);
        assert!(
            text.contains(&format!("  {:<6} {}", "CN1", "not measured")),
            "{text}"
        );
        assert!(
            text.contains(&format!("  {:<6} {}", "RDF1", "not applicable")),
            "{text}"
        );
        assert!(
            text.contains(&format!("  {:<6} {}", "URI1", "FAIL")),
            "{text}"
        );
        assert!(
            text.contains(&format!("  {:<6} {}", "VER2", "pass")),
            "{text}"
        );
    }
}
