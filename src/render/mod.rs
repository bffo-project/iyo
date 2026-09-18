//! Stage 6: turn the model into files.
//!
//! Every renderer reads the model and the layout plan, never RDF, with the one
//! exception of `rdf`, whose job is to serialise the triples the model came
//! from.

pub mod audit;
pub mod html;
pub mod jsonld;
pub mod llms;
pub mod manifest;
pub mod markdown;
pub mod rdf;
pub mod view;

use crate::config::Config;
use crate::load::Store;
use crate::model::{Document, Release, Term, TermKind};
use crate::site::{NamespacePlan, Plan, Rep};
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// What every renderer needs: the model, where things go, and the settings.
pub struct Ctx<'a> {
    pub release: &'a Release,
    pub store: &'a Store,
    pub plan: &'a Plan,
    pub config: &'a Config,
    /// What changed since the previous release, when one was given. The page
    /// needs it, not only the changelog file: a term that was removed has no
    /// page any more, so the anchor a link into the old release carries can
    /// only be answered on the document page.
    pub changes: Option<&'a crate::diff::Diff>,
}

impl<'a> Ctx<'a> {
    /// The snapshots planned for one namespace.
    ///
    /// Computed rather than stored so that a snapshot's own context, whose
    /// mount already carries the version segment, reports none: a release
    /// does not contain releases.
    pub fn snapshots(&self, ns: &NamespacePlan) -> Vec<crate::version::Snapshot> {
        let Some(policy) = crate::version::Policy::parse(&self.config.site.snapshots) else {
            return Vec::new();
        };
        let (all, _) = crate::version::plan(
            self.release,
            self.plan,
            policy,
            self.config.site.release.as_deref(),
        );
        all.into_iter()
            .filter(|s| s.namespace == ns.iri && !ns.mount.ends_with(&format!("{}/", s.segment)))
            .collect()
    }

    pub fn lang(&self) -> &str {
        &self.plan.lang
    }

    pub fn namespace_of(&self, term: &Term) -> Option<&'a NamespacePlan> {
        self.plan.namespace(&term.namespace)
    }

    /// The document a term is defined in.
    pub fn document_of(&self, term: &Term) -> Option<&'a Document> {
        term.defined_in
            .as_deref()
            .and_then(|iri| self.release.document(iri))
    }

    /// The plan for a document's own namespace, which is where its index lives.
    pub fn namespace_of_document(&self, doc: &Document) -> Option<&'a NamespacePlan> {
        self.plan
            .namespaces
            .iter()
            .find(|n| n.document.as_deref() == Some(doc.iri.as_str()))
            .or_else(|| self.plan.namespace(&doc.iri))
    }

    /// A CURIE when a prefix covers the IRI, otherwise the IRI.
    pub fn short(&self, iri: &str) -> String {
        crate::model::short(iri, &self.release.prefixes)
    }

    /// A Markdown link to a local term, or just the short form for anything
    /// this release does not publish.
    pub fn link_md(&self, iri: &str, from: &NamespacePlan) -> String {
        let label = self.short(iri);
        match self.release.term(iri) {
            Some(t) if !t.foreign => match self.namespace_of(t) {
                Some(ns) => {
                    let target = self.plan.term_url(ns, &t.local_name, Rep::Markdown);
                    let _ = from;
                    format!("[{label}]({target})")
                }
                None => label,
            },
            _ => label,
        }
    }
}

/// Turn `http://purl.org/adms/status/UnderDevelopment` into
/// "under development" for display, without inventing a vocabulary of labels.
pub fn humanise(iri: &str) -> String {
    let local = crate::vocab::local_name(iri);
    let mut out = String::new();
    for (i, c) in local.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    out.replace(['_', '-'], " ")
}

/// Staging directories beside `out` that this run did not make.
///
/// Each one is an interrupted build's leftovers, or a build running right
/// now. Naming them is safe; deleting them is not.
pub fn stale_staging(parent: &Utf8Path, name: &str, mine: &Utf8Path) -> Vec<String> {
    let prefix = format!("{name}.iyo-partial-");
    let dir = if parent.as_str().is_empty() {
        Utf8Path::new(".")
    } else {
        parent
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|e| Utf8PathBuf::from_path_buf(e.path()).ok())
        .filter(|p| p != mine && p.is_dir())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.starts_with(prefix.as_str()))
        })
        .map(|p| {
            format!(
                "{p} is a staging directory from an interrupted or concurrent build; \
                 nothing reads it, and it is safe to remove once no build is running"
            )
        })
        .collect();
    found.sort();
    found
}

/// One file the build produced.
#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub path: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub schema_version: &'static str,
    pub out_dir: String,
    /// Things the build decided not to write, and why. Empty on almost
    /// every run, which is why it is skipped rather than serialised as an
    /// empty array.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub files: Vec<Artifact>,
    /// SHA-256 over every path and its content, in path order. Two builds of
    /// the same inputs produce the same digest.
    pub digest: String,
    pub counts: BTreeMap<String, usize>,
}

/// Accumulates output in memory so the digest can be computed before anything
/// touches the disk, and so a failed build writes nothing.
#[derive(Default)]
pub struct Output {
    /// Paths a later stage will write, which the renderer does not produce.
    /// The auditor treats them as existing, so a page may link the PDF that
    /// Typst is about to compile without that reading as a broken link, and
    /// a page may not link anything else that is not there.
    pub promised: std::collections::BTreeSet<String>,
    files: BTreeMap<String, String>,
    /// Files that are not text: a theme's fonts, a raster logo. Kept apart
    /// from `files` because everything that reads the output as text --
    /// the auditor, `undefined_tokens`, the JSON-LD round-trip check --
    /// would have to decode them first and has nothing to say about them.
    /// They are written, hashed and audited for existence like any other
    /// file.
    blobs: BTreeMap<String, Vec<u8>>,
    /// Structural findings from auditing the rendered HTML.
    pub issues: Vec<audit::Issue>,
    /// The contrast gate's verdict on the theme's tokens.
    pub contrast_failures: usize,
    /// Errors from the gate that are not about one pair: a colour that cannot
    /// be parsed, or a token file declaring a schema this build does not read.
    /// They are counted apart from `contrast_failures` because a pair failure
    /// only stops a build when its scheme is published, and these are true
    /// whatever is published: the scheme they belong to was skipped entirely,
    /// so it contributes no pairs and would otherwise pass unnoticed.
    pub contrast_errors: usize,
    /// The pairs that failed, so a theme author is told which colours and by
    /// how much rather than only how many. The gate computed all of this and
    /// nothing but the count used to leave this struct, which made a theme
    /// unauthorable: "14 pairs fail" and no way to know which fourteen.
    pub contrast_failed: Vec<crate::theme::PairResult>,
    /// The gate's findings, so a refusal can name the token at fault rather
    /// than only counting it.
    pub contrast_findings: Vec<crate::theme::Finding>,
    /// Things the build decided not to write, and why. A refusal a reader
    /// would want to know about is not a finding -- the build is correct --
    /// but it is not nothing either, so it reaches the summary and `--json`
    /// rather than only `versions.json`.
    pub notes: Vec<String>,
}

impl Output {
    /// Findings that must stop a build.
    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.level == audit::Level::Error)
            .count()
    }
}

impl Output {
    /// Record a path a later stage will write.
    pub fn promise(&mut self, path: impl Into<String>) {
        self.promised.insert(path.into());
    }

    pub fn add(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    /// Add a file whose bytes are not text.
    pub fn add_bytes(&mut self, path: impl Into<String>, content: Vec<u8>) {
        self.blobs.insert(path.into(), content);
    }

    /// Every path this build will write, text and binary alike, in one
    /// order. The auditor needs it to tell a link to a font from a broken
    /// one, and the digest needs it to be stable.
    fn all_paths(&self) -> BTreeMap<&str, &[u8]> {
        self.files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_bytes()))
            .chain(self.blobs.iter().map(|(p, c)| (p.as_str(), c.as_slice())))
            .collect()
    }

    pub fn digest(&self) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha256::new();
        // Text and binary in one path order, so a build with no binary
        // files hashes exactly as it did before they were possible.
        for (path, content) in self.all_paths() {
            hasher.update(path.as_bytes());
            hasher.update([0u8]);
            hasher.update(content);
            hasher.update([0u8]);
        }
        let mut out = String::with_capacity(64);
        for byte in hasher.finalize() {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// The rendered files, for tests and for callers that do not write to disk.
    pub fn into_files(self) -> BTreeMap<String, String> {
        self.files
    }

    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    /// Output paths that are the same file on a case-insensitive filesystem.
    ///
    /// macOS and Windows fold case, so two such paths silently become one and
    /// the build writes fewer files than it reports. A site with both cannot
    /// be served from a case-insensitive host either, so this is a defect
    /// wherever the build runs, not a local inconvenience.
    pub fn case_collisions(&self) -> Vec<(String, String)> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut out = Vec::new();
        for path in self.all_paths().keys() {
            if let Some(first) = seen.insert(path.to_lowercase(), (*path).to_owned()) {
                out.push((first, (*path).to_owned()));
            }
        }
        out
    }

    /// Stage the whole build in a sibling directory and swap it into place,
    /// so `out_dir` is at every instant either wholly the previous build or
    /// wholly this one, never a mix of the two (clig.dev G23). An
    /// interrupted or failing write leaves `out_dir` untouched, and a
    /// rebuild that produces fewer files than the last one leaves no
    /// orphaned page behind, because the new directory contains only what
    /// this build produced.
    ///
    /// The pid in the staging and aside names never reaches a file's
    /// content, only a path on disk, so it cannot perturb the digest (which
    /// is computed from `self.files` before this runs) or the byte-for-byte
    /// reproducibility of two builds of the same release.
    /// The artifacts a `write` would produce, without producing them.
    ///
    /// `--dry-run` used to report an empty list and a count of zero while
    /// still printing the correct digest, so "report what would be written"
    /// reported everything except that. Same paths, same sizes, same order as
    /// `write`, from the same map.
    pub fn planned(&self) -> Vec<Artifact> {
        self.all_paths()
            .into_iter()
            .map(|(path, content)| Artifact {
                path: path.to_owned(),
                bytes: content.len(),
            })
            .collect()
    }

    pub fn write(self, out_dir: &Utf8Path) -> Result<Vec<Artifact>> {
        let collisions = self.case_collisions();
        if let Some((a, b)) = collisions.first() {
            anyhow::bail!(
                "{} output paths differ only by case, so a case-insensitive \
                 filesystem would keep one and lose the other; the first pair \
                 is {a} and {b}",
                collisions.len() * 2
            );
        }
        if out_dir.is_file() {
            return Err(crate::Failure::err(
                crate::exit::IO,
                format!(
                    "{out_dir} exists and is a regular file; the build writes a directory there"
                ),
                format!("pass --out with a directory path, or remove {out_dir}"),
            ));
        }
        // `exists()` follows a symlink but `rename` does not: swapping a
        // symlinked `<out>` would move the link itself aside and leave a
        // real directory standing in for it, so whatever it used to point
        // at -- a normal deployment shape -- would silently stop being
        // updated while the build kept exiting 0. Refuse instead of
        // guessing which directory the caller meant.
        if let Ok(meta) = std::fs::symlink_metadata(out_dir)
            && meta.file_type().is_symlink()
        {
            anyhow::bail!(
                "{out_dir} is a symlink; refusing to replace it because the swap \
                 moves the link aside and puts a real directory in its place, so \
                 whatever it points at would silently stop being updated. Point \
                 --out at the real directory instead."
            );
        }

        let pid = std::process::id();
        // Derive both sibling names from path components, not `Display`:
        // camino's `Display` is verbatim, so a trailing slash on `out_dir`
        // (routine after shell tab-completion on a directory) would make
        // `format!("{out_dir}.iyo-partial-{pid}")` a *child* of `out_dir`
        // rather than a sibling, and `create_dir_all` would then create
        // `out_dir` itself as a side effect before the swap ever runs.
        let parent = out_dir.parent().unwrap_or(Utf8Path::new(""));
        let name = out_dir
            .file_name()
            .context("--out has no final path component")?;
        let staging = parent.join(format!("{name}.iyo-partial-{pid}"));
        let old = parent.join(format!("{name}.iyo-old-{pid}"));

        // A crash or a `kill -9` under the same pid (pid reuse aside) can
        // leave a staging directory behind; start clean rather than merging
        // into it. An interrupt no longer leaves one: the handler installed
        // in `cli::main` turns SIGINT into a flag, `write_into` checks it
        // between files, and the error path below removes the directory.
        if staging.exists() {
            std::fs::remove_dir_all(&staging)
                .with_context(|| format!("removing stale staging directory {staging}"))?;
        }
        // Another run's leftovers are not this run's to delete -- a
        // concurrent build's staging directory looks exactly the same -- so
        // they are named instead. What can still leave one is a signal that
        // cannot be caught, or a power cut; naming them is how that litter
        // stays bounded.
        for note in stale_staging(parent, name, &staging) {
            eprintln!("note: {note}");
        }
        std::fs::create_dir_all(&staging).with_context(|| format!("creating {staging}"))?;

        let artifacts = match self.write_into(&staging) {
            Ok(artifacts) => artifacts,
            Err(e) => {
                // Failure before the swap: discard the staging directory and
                // leave out_dir exactly as it was.
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
        };

        if !out_dir.exists() {
            // The atomic case: a single rename, so out_dir either does not
            // exist yet or is wholly the new build. There is no window.
            std::fs::rename(&staging, out_dir)
                .with_context(|| format!("renaming {staging} into place as {out_dir}"))?;
        } else {
            // Two renames: out_dir is briefly absent from the filesystem
            // between them (never a mix of old and new content), then
            // wholly the new build. A lookup landing in that gap sees
            // ENOENT, not corruption; the gap is two syscalls wide with no
            // computation between them.
            if let Err(e) = std::fs::rename(out_dir, &old) {
                // Nothing left behind on failure: the staged build is
                // discarded, same as the pre-swap failure path above. A
                // leftover `old` here is never the current out_dir (that
                // rename is what just failed) -- at worst it is debris from
                // an earlier same-pid crash that never got past this same
                // step, so clear it too. Left alone it would wedge every
                // later build under this pid with ENOTEMPTY forever.
                let _ = std::fs::remove_dir_all(&staging);
                let _ = std::fs::remove_dir_all(&old);
                return Err(e).with_context(|| format!("renaming {out_dir} aside to {old}"));
            }
            if let Err(e) = std::fs::rename(&staging, out_dir) {
                // The rename completing the swap failed after the old build
                // was already moved aside: put it back so out_dir is not
                // left missing, and drop the now-orphaned staged copy.
                let _ = std::fs::rename(&old, out_dir);
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e)
                    .with_context(|| format!("renaming {staging} into place as {out_dir}"));
            }
            if let Err(e) = std::fs::remove_dir_all(&old) {
                // The swap already succeeded; a leftover aside directory is
                // an untidy disk, not a broken build.
                eprintln!("warning: could not remove the previous build at {old}: {e}");
            }
        }

        Ok(artifacts)
    }

    /// Write every file into `dir`, exactly as `write` always has. Split out
    /// so `write` can point it at a staging directory first.
    fn write_into(&self, dir: &Utf8Path) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();
        for (path, content) in self.all_paths() {
            // Between files is where an interrupt is safe to act on: the
            // staging directory is this run's alone, and `write`'s error
            // path removes it, so stopping here leaves `--out` untouched
            // (clig.dev G23). A reference build writes about a thousand
            // small files, so the gap between checks is sub-millisecond.
            crate::interrupt::check()?;
            let full: Utf8PathBuf = dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating {parent}"))?;
            }
            std::fs::write(&full, content).with_context(|| format!("writing {full}"))?;
            artifacts.push(Artifact {
                path: path.to_owned(),
                bytes: content.len(),
            });
        }
        Ok(artifacts)
    }
}

/// Every machine-readable file of one namespace.
///
/// Split out so that a snapshot can call it with a plan whose mount carries a
/// version segment, which is what makes a snapshot a copy of the release
/// rather than a second implementation of it.
fn namespace_files(
    ctx: &Ctx<'_>,
    ns: &NamespacePlan,
    context: &jsonld::Context,
    out: &mut Output,
) -> Result<()> {
    let document = ns
        .document
        .as_deref()
        .and_then(|iri| ctx.release.document(iri));

    // The namespace's own graph, from the file its document came from.
    if let Some(doc) = document {
        out.add(
            ctx.plan.document_path(ns, Rep::Turtle),
            rdf::document(ctx.store, doc.source_file, &ctx.release.prefixes)?,
        );
        let path = ctx.plan.document_path(ns, Rep::Markdown);
        let body = markdown::relativise_links(
            &markdown::document(ctx, ns, doc),
            &path,
            &ctx.plan.base_url,
        );
        out.add(path, body);
        out.add(
            ctx.plan.document_path(ns, Rep::JsonLd),
            jsonld::document(ctx, context, doc.source_file)?,
        );
    }
    out.add(
        format!("{}context.jsonld", ns.mount),
        jsonld::namespace_context(ctx, context, ns)?,
    );

    out.add(ctx.plan.llms_path(ns), llms::namespace(ctx, ns, document));
    out.add(
        format!("{}terms.json", ns.mount),
        serde_json::to_string_pretty(&term_index(ctx, Some(ns)))?,
    );

    let frontmatter =
        markdown::FrontMatter::parse(&ctx.config.site.md_frontmatter).with_context(|| {
            format!(
                "unknown md-frontmatter option {:?}",
                ctx.config.site.md_frontmatter
            )
        })?;

    // `.enumerate()` after the namespace filter, not before: `weight` is a
    // 1-based position among this namespace's own terms, in the same
    // alphabetical-by-IRI order `Release::local_terms` yields and the loop
    // below writes files in, not a global position across every namespace.
    let local = ctx.release.local_terms().filter(|t| t.namespace == ns.iri);
    for (i, term) in local.enumerate() {
        let path = ctx.plan.term_path(ns, &term.local_name, Rep::Markdown);
        let body =
            markdown::relativise_links(&markdown::term(ctx, ns, term)?, &path, &ctx.plan.base_url);
        // Front matter is prefixed after relativisation, never before, so
        // rewriting link targets never sees or mangles it.
        let front = markdown::term_front_matter(
            frontmatter,
            term.display(ctx.lang()),
            &term.iri,
            term.kind.label(),
            i + 1,
            &ctx.plan.term_request_path(ns, &term.local_name),
        );
        out.add(path, format!("{front}{body}"));
        out.add(
            ctx.plan.term_path(ns, &term.local_name, Rep::Turtle),
            rdf::term(ctx.store, &term.iri, &ctx.release.prefixes)?,
        );
        out.add(
            ctx.plan.term_path(ns, &term.local_name, Rep::JsonLd),
            jsonld::term(ctx, context, &term.iri)?,
        );
    }

    Ok(())
}

/// The template and the data a PDF is compiled from.
///
/// Both are part of the build, so they are deterministic and hashed with
/// everything else; running Typst over them is a separate step because it
/// needs a program the build cannot assume is installed.
fn pdf_inputs(
    ctx: &Ctx<'_>,
    ns: &NamespacePlan,
    theme_dir: Option<&camino::Utf8Path>,
    out: &mut Output,
) -> Result<()> {
    let Some(doc) = ns
        .document
        .as_deref()
        .and_then(|iri| ctx.release.document(iri))
    else {
        return Ok(());
    };
    let dv = view::document(ctx, ns, doc)?;
    out.add(
        format!("{}pdf/model.json", ns.mount),
        serde_json::to_string_pretty(&serde_json::json!({
            "site": {
                "title": view::site(ctx).title,
                "base_url": ctx.plan.base_url,
                "lang": ctx.lang(),
            },
            "document": dv,
            // The name the compiled file must take. Guessing it from the
            // directory picks a term's Turtle over the namespace's, and the
            // guess is not even stable across runs.
            "pdf": { "stem": ns.stem },
        }))?,
    );
    out.add(
        format!("{}pdf/spec.typ", ns.mount),
        html::pdf_template(theme_dir),
    );
    // Typst writes this, not the renderer, but a page may link it.
    out.promise(format!("{}{}.pdf", ns.mount, ns.stem));
    Ok(())
}

/// Cache policy a host adapter should apply. A snapshot never changes, so it
/// can be cached forever; latest moves with every release, so it cannot. This
/// is the inverse of what DCMI serves today.
const SNAPSHOT_CACHE: &str = "public, max-age=31536000, immutable";
const LATEST_CACHE: &str = "public, max-age=300, must-revalidate";

/// The version links, as RDF, in a file of their own.
///
/// The convention asks for `dcterms:hasVersion` in the latest `index.ttl`.
/// That would mean adding triples to the publisher's own graph, and this tool
/// publishes the RDF it was given. The links go in a sibling instead, named
/// in `llms.txt` and on the page, so the statement is made without anything
/// being put into the publisher's mouth.
fn versions_ttl(base: &str, snapshots: &[&crate::version::Snapshot]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "@prefix dcterms: <{}> .", crate::vocab::DCTERMS);
    let _ = writeln!(s, "@prefix owl: <{}> .", crate::vocab::OWL);
    for snap in snapshots {
        let _ = writeln!(s, "\n<{base}> dcterms:hasVersion <{}> .", snap.url);
        let _ = writeln!(s, "\n<{}>", snap.url);
        let _ = writeln!(s, "    dcterms:isVersionOf <{base}> ;");
        let _ = writeln!(s, "    owl:versionInfo {:?} .", snap.segment);
    }
    s
}

/// Render every target into memory.
pub fn render(ctx: &Ctx<'_>) -> Result<Output> {
    render_with(ctx, None)
}

/// Render every target, optionally overlaying a theme directory and writing a
/// changelog against a previous release.
pub fn render_with(ctx: &Ctx<'_>, theme_dir: Option<camino::Utf8PathBuf>) -> Result<Output> {
    let mut out = Output::default();

    // Whole-release serialisation: the graph a validator needs.
    out.add(
        "release.ttl",
        rdf::release(ctx.store, &ctx.release.prefixes)?,
    );

    // One context for the release, from which each namespace publishes the
    // part it uses and each term file inlines the part it needs.
    let context = jsonld::context(ctx);
    out.add("release.jsonld", jsonld::release(ctx, &context)?);
    out.add("context.jsonld", jsonld::context_document(&context)?);

    for ns in &ctx.plan.namespaces {
        namespace_files(ctx, ns, &context, &mut out)?;
    }

    // Site-level files.
    out.add("llms.txt", llms::root(ctx));
    out.add(
        "terms.json",
        serde_json::to_string_pretty(&term_index(ctx, None))?,
    );
    out.add(
        "manifest.json",
        serde_json::to_string_pretty(&manifest::build(ctx))?,
    );
    out.add("llms-full.txt", llms::full(ctx)?);

    // HTML last: it is the only stage that needs the theme, and rendering it
    // after the machine artefacts means a template error cannot leave a
    // half-written site behind.
    let tokens = crate::theme::tokens_for(theme_dir.as_deref())?;
    let contrast = crate::theme::gate(&tokens);
    // Only the schemes this build publishes can stop it. A pair in a palette
    // the site never serves is a failure earned by nothing, which is the
    // shape of defect this project keeps finding in its own checks. The
    // unpublished palette is still checked, and reported as a note, so that
    // turning `color_scheme` back to `auto` is not a surprise.
    let published = ctx.plan.color_scheme.published();
    let (shipped, unshipped): (Vec<_>, Vec<_>) = contrast
        .pairs
        .iter()
        .filter(|p| !p.passes)
        .cloned()
        .partition(|p| published.contains(&p.scheme));
    out.contrast_failures = shipped.len();
    out.contrast_failed = shipped;
    // Pair failures carry a `theme.contrast-*` id; anything else at error
    // level is a fault in the token file itself.
    out.contrast_errors = contrast
        .findings
        .iter()
        .filter(|f| {
            f.severity == crate::theme::Severity::Error && !f.rule.starts_with("theme.contrast-")
        })
        .count();
    out.contrast_findings = contrast.findings.clone();
    if !unshipped.is_empty() {
        out.notes.push(format!(
            "{} colour pair{} fail in the {} scheme, which this build does not \
             publish (site.color_scheme). They would stop a build that did.",
            unshipped.len(),
            if unshipped.len() == 1 { "" } else { "s" },
            unshipped.first().map(|p| p.scheme).unwrap_or("other"),
        ));
    }
    let theme_for_pdf = theme_dir.clone();
    let env = html::environment(theme_dir.clone());
    html::render(ctx, &env, &tokens, theme_dir.as_deref(), &mut out)?;

    // Versioned snapshots. A snapshot is the same namespace rendered against
    // a plan whose mount carries the version segment, so it is a copy of the
    // release rather than a second design, and its links stay inside it.
    let policy = crate::version::Policy::parse(&ctx.config.site.snapshots)
        .with_context(|| format!("unknown snapshots option {:?}", ctx.config.site.snapshots))?;
    let (snapshots, refused) = crate::version::plan(
        ctx.release,
        ctx.plan,
        policy,
        ctx.config.site.release.as_deref(),
    );
    out.notes.extend(refused.iter().cloned());
    for snap in &snapshots {
        let mut snapshot_plan = ctx.plan.clone();
        let Some(ns) = snapshot_plan
            .namespaces
            .iter_mut()
            .find(|n| n.iri == snap.namespace)
        else {
            continue;
        };
        ns.mount = format!("{}{}/", ns.mount, snap.segment);
        // Nothing nests under a snapshot, so it reserves nothing.
        ns.reserved = Vec::new();
        let ns = ns.clone();
        let snapshot_ctx = Ctx {
            release: ctx.release,
            store: ctx.store,
            plan: &snapshot_plan,
            config: ctx.config,
            // A release does not carry a changelog of what came after it.
            changes: None,
        };
        namespace_files(&snapshot_ctx, &ns, &context, &mut out)?;
        html::namespace(&snapshot_ctx, &ns, &env, &mut out)?;
        // An archived release is self-contained, so it carries its own PDF
        // rather than linking the one the newest release will overwrite.
        if ctx.config.site.pdf {
            pdf_inputs(&snapshot_ctx, &ns, theme_for_pdf.as_deref(), &mut out)?;
        }
    }

    // Latest-only files: the version links, and the record of what exists.
    for ns in &ctx.plan.namespaces {
        let mine: Vec<&crate::version::Snapshot> =
            snapshots.iter().filter(|s| s.namespace == ns.iri).collect();
        if mine.is_empty() {
            continue;
        }
        let base = ctx.plan.document_url(ns, Rep::Html);
        out.add(
            format!("{}versions.ttl", ns.mount),
            versions_ttl(&base, &mine),
        );
    }
    out.add(
        "versions.json",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": crate::model::SCHEMA_VERSION,
            "generator": format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            "policy": policy.as_str(),
            // Whether `B/` is the newest release or a document that moves on
            // after one. It changes no bytes here; a reader of the archive
            // needs to know which promise is being made.
            "latest": ctx.config.site.latest,
            "cache_control": {
                "snapshot": SNAPSHOT_CACHE,
                "latest": LATEST_CACHE,
            },
            "namespaces": ctx.plan.namespaces.iter().map(|ns| serde_json::json!({
                "iri": ns.iri,
                "latest_url": ctx.plan.document_url(ns, Rep::Html),
                "versions": snapshots.iter().filter(|s| s.namespace == ns.iri).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "not_written": refused,
        }))?,
    );

    // Every custom property a stylesheet reads must be defined, or the
    // declaration is dropped and the element keeps a browser default while
    // the contrast gate still reports success. This turns that silent failure
    // into a build error, for the bundled theme and for an overlay alike.
    {
        let tokens_css = out
            .files
            .get("assets/tokens.css")
            .cloned()
            .unwrap_or_default();
        let sheets: Vec<&str> = out
            .files
            .iter()
            .filter(|(p, _)| p.ends_with(".css") && p.as_str() != "assets/tokens.css")
            .map(|(_, c)| c.as_str())
            .collect();
        let missing = crate::theme::undefined_tokens(&sheets, &tokens_css);
        if !missing.is_empty() {
            anyhow::bail!(
                "the theme reads {} custom {} that tokens.css does not define: {}",
                missing.len(),
                if missing.len() == 1 {
                    "property"
                } else {
                    "properties"
                },
                missing.join(", ")
            );
        }
    }

    // The changelog, one per namespace, carrying only the changes that
    // namespace's own terms account for.
    if let Some(diff) = ctx.changes {
        for ns in &ctx.plan.namespaces {
            let mine: Vec<crate::diff::Change> = diff
                .changes
                .iter()
                .filter(|c| c.namespace.as_deref() == Some(ns.iri.as_str()))
                .cloned()
                .collect();
            if mine.is_empty() {
                continue;
            }
            let title = ns
                .document
                .as_deref()
                .and_then(|iri| ctx.release.document(iri))
                .map(|d| d.display(ctx.lang()).to_owned())
                .unwrap_or_else(|| ns.iri.clone());
            let scoped = crate::diff::Diff {
                scope: Some(ns.iri.clone()),
                breaking: mine
                    .iter()
                    .filter(|c| c.severity == crate::diff::Severity::Breaking)
                    .count(),
                changes: mine,
                ..diff.clone()
            };
            out.add(
                format!("{}changes.md", ns.mount),
                crate::diff::markdown(&scoped, &title),
            );
        }
    }

    // PDF inputs. The template and the data are part of the build so that
    // they are deterministic and hashed with everything else; running Typst
    // over them is a separate step, because it needs a program the build
    // cannot assume is installed.
    if ctx.config.site.pdf {
        for ns in &ctx.plan.namespaces {
            pdf_inputs(ctx, ns, theme_for_pdf.as_deref(), &mut out)?;
        }
    }

    // Host adapters, compiled from the manifest and from nothing else. The
    // manifest itself is always written, because it is the artefact; a host
    // configuration is a projection a publisher asks for by name.
    let manifest = manifest::build(ctx);
    for name in &ctx.config.site.hosts {
        let host =
            crate::adapter::Host::parse(name).with_context(|| format!("unknown host {name:?}"))?;
        for (path, content) in crate::adapter::emit(&manifest, host) {
            out.add(path, content);
        }
    }

    // Audit what was just rendered. These are the checks that catch the
    // defects measured on the deployment this tool replaces, and they run on
    // every build so a theme cannot regress them silently.
    // Binary files count as existing: a stylesheet referencing a theme's
    // font must not read as a broken link just because the auditor cannot
    // parse a woff2.
    let mut known: std::collections::BTreeSet<String> =
        out.all_paths().keys().map(|p| (*p).to_owned()).collect();
    known.extend(out.promised.iter().cloned());
    // Root-absolute links are relative to the server root; the output tree is
    // the site root, which sits below it whenever the site is served from a
    // path. The auditor needs the same value the templates used.
    let base_path = crate::site::base_path(ctx.config);
    let mut issues = Vec::new();
    for (path, content) in &out.files {
        if path.ends_with(".html") {
            issues.extend(audit::page(
                path,
                content,
                &known,
                &ctx.plan.base_url,
                &base_path,
                &ctx.config.site.external_paths,
            ));
        } else if path.ends_with(".md") {
            issues.extend(audit::markdown(
                path,
                content,
                &known,
                &ctx.plan.base_url,
                &base_path,
            ));
        }
    }
    issues.sort_by(|a, b| (b.level, &a.rule, &a.page).cmp(&(a.level, &b.rule, &b.page)));

    out.add(
        "a11y/structure.json",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": crate::model::SCHEMA_VERSION,
            "generator": format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            "pages_checked": out.files.keys().filter(|p| p.ends_with(".html")).count(),
            "errors": issues.iter().filter(|i| i.level == audit::Level::Error).count(),
            "warnings": issues.iter().filter(|i| i.level == audit::Level::Warning).count(),
            "issues": issues,
        }))?,
    );
    out.add(
        "a11y/contrast.json",
        serde_json::to_string_pretty(&contrast)?,
    );
    out.issues = issues;

    Ok(out)
}

/// A flat index of local terms, shaped so that a stateless search or fetch
/// service could be built on it without a database.
#[derive(Debug, Serialize)]
pub struct IndexEntry {
    pub iri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curie: Option<String>,
    pub kind: TermKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    pub deprecated: bool,
    pub namespace: String,
    pub html: String,
    pub md: String,
    pub ttl: String,
}

pub fn term_index(ctx: &Ctx<'_>, only: Option<&NamespacePlan>) -> Vec<IndexEntry> {
    let lang = ctx.lang();
    ctx.release
        .local_terms()
        .filter(|t| only.is_none_or(|ns| t.namespace == ns.iri))
        .filter_map(|t| {
            let ns = ctx.namespace_of(t)?;
            Some(IndexEntry {
                iri: t.iri.clone(),
                curie: t.curie.clone(),
                kind: t.kind,
                label: t.display(lang).to_owned(),
                definition: t.summary(lang),
                deprecated: t.deprecated,
                namespace: t.namespace.clone(),
                html: ctx.plan.term_url(ns, &t.local_name, Rep::Html),
                md: ctx.plan.term_url(ns, &t.local_name, Rep::Markdown),
                ttl: ctx.plan.term_url(ns, &t.local_name, Rep::Turtle),
            })
        })
        .collect()
}
