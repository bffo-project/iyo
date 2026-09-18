//! The conformance gate: does a live origin implement the negotiation rules?
//!
//! Separate from `probe`, which asks whether every published IRI resolves.
//! That is exhaustive and slow (1,148 requests and 21 minutes against the
//! BFFO origin); this is fixed and fast, because a gate nobody can afford to
//! run in CI is not a gate. See `docs/ci.md`.

pub mod cases;
pub mod judge;
pub mod resolve;

use crate::http;
use crate::render::manifest::Manifest;
use anyhow::Result;
use resolve::{Resolved, ResolvedExpect};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::{Duration, Instant};

/// The outcome of one run against one origin: what was checked, and how
/// much of the contract never got the chance to be.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    /// The version of this document's shape, first field so a consumer can
    /// branch on it before anything else (`docs/cli.md`, "`--json`").
    pub schema_version: &'static str,
    /// `origin` with any userinfo password masked (`user:***@host`): this
    /// is what gets sent to the wire, not what gets reported.
    pub origin: String,
    pub verdicts: Vec<judge::Verdict>,
    /// Roles that found no subject on this manifest. Each one is a case that
    /// did not run at all.
    pub unresolved: Vec<String>,
    pub passed: usize,
    pub failed: usize,
    /// How many cases actually ran before the deadline (or the plan's end,
    /// whichever came first).
    pub made: usize,
    /// Whether the deadline was reached before every resolved case ran.
    pub truncated: bool,
}

impl Report {
    /// A run is conformant only when nothing failed, every role resolved,
    /// *and* the run was not cut off by the deadline. A run where every case
    /// that ran passed but a role found no subject is NOT conformant: the
    /// cases that did not run reported nothing, and a case that silently did
    /// not run is worse than one that is missing, because it reports
    /// success. A deadline cutoff is the same shape of gap: the cases past
    /// it never ran, and treating that as "conformant, just smaller" is
    /// exactly the silent success this method exists to refuse.
    pub fn conformant(&self) -> bool {
        self.failed == 0 && self.unresolved.is_empty() && !self.truncated
    }
}

/// Run the bundled contract against `manifest`, sending every request to
/// `origin`. Fetches in batches of `http::BATCH`, with bodies, because
/// `serve` expectations must see the identity IRI to tell a real term page
/// from an application's catch-all route.
///
/// A header naming the origin and how many cases are planned goes to
/// stderr before the first request is made (clig.dev G22), and on a TTY
/// only, a progress line updates as batches complete. `deadline` bounds the
/// whole run's wall-clock time; once it is reached the run stops and
/// returns what it gathered.
pub fn run(manifest: &Manifest, origin: &str, timeout: u32, deadline: Duration) -> Result<Report> {
    if !http::available() {
        return Err(crate::Failure::err(
            crate::exit::ENVIRONMENT,
            "curl was not found on PATH",
            "`iyo conform` shells out to curl to make requests; install curl, or use \
             `--cases FILE` which writes the plan without making any",
        ));
    }
    let contract = cases::bundled()?;
    let plan = resolve::resolve(&contract, manifest);
    eprintln!(
        "conform: {} case{} planned for {}",
        plan.cases.len(),
        if plan.cases.len() == 1 { "" } else { "s" },
        http::redact_url(origin)
    );

    let started = Instant::now();
    let show_progress = http::stderr_is_terminal();
    let mut verdicts = Vec::with_capacity(plan.cases.len());
    let mut made = 0usize;
    let mut truncated = false;

    for batch in plan.cases.chunks(http::BATCH) {
        // Same as the deadline: the cases past this point never ran, and a
        // run that did not finish is not conformant (`Report::conformant`).
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
            .map(|resolved| http::Request {
                url: request_url(origin, resolved),
                accept: resolved.accept.clone(),
            })
            .collect();
        // See the matching comment in `probe::run`: without this, a batch
        // already under way can run for `chunk_depth × timeout`, which is
        // more than `deadline`'s default of 60s at the defaults BATCH=40,
        // CONCURRENCY=8, timeout=15s (75s) -- so a slow but working origin
        // could truncate on the very first batch.
        let batch_timeout = http::clamped_timeout(deadline - elapsed, batch.len(), timeout);
        let replies = http::fetch(&requests, batch_timeout, true)?;
        for (resolved, reply) in batch.iter().zip(replies.iter()) {
            verdicts.push(judge::judge(resolved, reply));
        }
        made += batch.len();
        if show_progress {
            eprint!("\rconform: {made}/{} cases", plan.cases.len());
            let _ = std::io::stderr().flush();
        }
    }
    if show_progress {
        eprintln!();
    }

    let passed = verdicts.iter().filter(|v| v.passed).count();
    let failed = verdicts.len() - passed;

    Ok(Report {
        schema_version: crate::model::SCHEMA_VERSION,
        origin: http::redact_url(origin),
        verdicts,
        unresolved: plan.unresolved,
        passed,
        failed,
        made,
        truncated,
    })
}

/// The URL one resolved case is checked at: `origin`, joined with the
/// site-root-relative path `resolve` computed, and the query string a
/// content-negotiation case asks for.
fn request_url(origin: &str, resolved: &Resolved) -> String {
    let base = origin.trim_end_matches('/');
    match &resolved.query {
        Some(query) => format!("{base}{}?{query}", resolved.path),
        None => format!("{base}{}", resolved.path),
    }
}

/// What a passing reply would have looked like, for the "got X, wanted Y"
/// line. The counterpart to what `judge` already computed as `got`: that is
/// what came back, this is what should have.
fn wanted(expect: &ResolvedExpect) -> String {
    match expect {
        ResolvedExpect::Absent => "404".to_owned(),
        ResolvedExpect::Serve { media_type, .. } => format!("200 {media_type}"),
        ResolvedExpect::File { media_type, .. } => format!("200 {media_type}"),
        ResolvedExpect::Redirect {
            location, status, ..
        } => format!("{status} -> {location}"),
    }
}

/// The report, for a person. Grouped by rule rather than by namespace, so
/// forty cases read as a handful of tallies instead of a flat list of forty
/// lines.
pub fn summary(report: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let namespaces: BTreeSet<&str> = report
        .verdicts
        .iter()
        .map(|v| v.case.namespace.as_str())
        .collect();
    let _ = writeln!(
        s,
        "{} cases, {} namespaces",
        report.verdicts.len(),
        namespaces.len(),
    );
    // Its own line, not folded into the header above: a role that did not
    // resolve is a case that silently did not run, and reporting that as
    // one more comma in a header is exactly how a silent gap stays silent.
    let _ = writeln!(
        s,
        "{} role{} did not resolve",
        report.unresolved.len(),
        if report.unresolved.len() == 1 {
            ""
        } else {
            "s"
        },
    );
    for name in &report.unresolved {
        let _ = writeln!(s, "  - {name}");
    }
    s.push('\n');

    if report.truncated {
        let _ = writeln!(
            s,
            "stopped early: the deadline was reached after {} case{}\n",
            report.made,
            if report.made == 1 { "" } else { "s" }
        );
    }

    // Grouped by the contract's own `group` field, in the order groups
    // first appear, so the tally reads in the contract's own order rather
    // than an alphabetisation nobody asked for.
    let mut groups: Vec<(&str, usize, usize)> = Vec::new();
    for v in &report.verdicts {
        let group = v.case.group.as_str();
        match groups.iter_mut().find(|(g, _, _)| *g == group) {
            Some(entry) => {
                entry.2 += 1;
                if v.passed {
                    entry.1 += 1;
                }
            }
            None => groups.push((group, usize::from(v.passed), 1)),
        }
    }
    let width = groups.iter().map(|(g, _, _)| g.len()).max().unwrap_or(0);
    for (group, passed, total) in &groups {
        let _ = writeln!(s, "  {group:<width$}  {passed}/{total}");
    }
    s.push('\n');

    for v in report.verdicts.iter().filter(|v| !v.passed) {
        let _ = writeln!(s, "FAIL  {}", v.case.name);
        let accept = v.case.accept.as_deref().unwrap_or("(none)");
        let _ = writeln!(s, "      {}  Accept: {accept}", v.case.path);
        let _ = writeln!(s, "      got {}, wanted {}", v.got, wanted(&v.case.expect));
        if let Some(consequence) = &v.consequence {
            let _ = writeln!(s, "      \u{2192} {consequence}");
        }
        s.push('\n');
    }

    let _ = writeln!(s, "{} passed, {} failed", report.passed, report.failed);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_roles_fail_the_run_rather_than_shrinking_it() {
        // A case that silently did not run is worse than one that is
        // missing, because it reports success.
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "https://example.org".to_owned(),
            verdicts: Vec::new(),
            unresolved: vec!["a term inside a release".to_owned()],
            passed: 40,
            failed: 0,
            made: 40,
            truncated: false,
        };
        assert!(!report.conformant());
        assert!(summary(&report).contains("1 role did not resolve"));
    }

    #[test]
    fn a_conformant_run_has_nothing_unresolved_and_nothing_failed() {
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "https://example.org".to_owned(),
            verdicts: Vec::new(),
            unresolved: Vec::new(),
            passed: 12,
            failed: 0,
            made: 12,
            truncated: false,
        };
        assert!(report.conformant());
        assert!(summary(&report).contains("0 roles did not resolve"));
    }

    #[test]
    fn a_run_the_deadline_cut_off_is_not_conformant_even_with_nothing_failed() {
        // The same shape of gap as an unresolved role: cases past the
        // deadline never ran, and reporting that as "conformant, just
        // fewer cases" is exactly the silent success `conformant` exists to
        // refuse.
        let report = Report {
            schema_version: crate::model::SCHEMA_VERSION,
            origin: "https://example.org".to_owned(),
            verdicts: Vec::new(),
            unresolved: Vec::new(),
            passed: 12,
            failed: 0,
            made: 12,
            truncated: true,
        };
        assert!(!report.conformant());
        assert!(summary(&report).contains("stopped early"));
    }
}
