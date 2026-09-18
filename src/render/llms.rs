//! `llms.txt`, one per namespace plus a root index.
//!
//! Structure follows the llms.txt v2 shape: an H1, a blockquote summary, free
//! Markdown, then H2 file lists whose entries are `[name](url): note`, with
//! Optional last. The content is what a vocabulary profile of the
//! convention to carry: how to use the vocabulary, a line per term linking its
//! Markdown sibling, the serialisations with Turtle first, and the reciprocal
//! link to a companion dataset.

use super::Ctx;
use crate::model::{Document, TermKind};
use crate::site::{NamespacePlan, Rep};
use anyhow::Result;
use std::fmt::Write;

/// The agent index for one namespace.
pub fn namespace(ctx: &Ctx<'_>, ns: &NamespacePlan, doc: Option<&Document>) -> String {
    let lang = ctx.lang();
    let mut out = String::new();

    let title = doc
        .map(|d| d.display(lang).to_owned())
        .unwrap_or_else(|| ns.iri.clone());
    let _ = writeln!(out, "# {title}\n");

    // Blockquote: purpose, then the facts an agent needs to write data.
    let mut summary = doc
        .and_then(|d| d.description(lang))
        .map(|d| d.value.replace(['\n', '\r'], " "))
        .unwrap_or_else(|| format!("Vocabulary published at {}", ns.iri));
    summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut facts = vec![format!("namespace {}", ns.iri)];
    if let Some(p) = &ns.prefix {
        facts.push(format!("prefix {p}"));
    }
    if let Some(d) = doc {
        if let Some(v) = &d.header.version_info {
            facts.push(format!("version {v}"));
        }
        if let Some(s) = &d.header.status {
            facts.push(super::humanise(s));
        }
        if let Some(l) = &d.header.license {
            facts.push(format!("licence {l}"));
        }
    }
    let _ = writeln!(out, "> {summary} ({}).\n", facts.join(", "));

    // Terms are grouped by the document that defines them, not by the
    // namespace they are named in: a shape named in the ontology namespace but
    // defined in the shapes file belongs on the shapes index, while its URL
    // still follows its own namespace.
    let terms: Vec<&crate::model::Term> = match doc {
        Some(d) => d
            .terms
            .iter()
            .filter_map(|iri| ctx.release.term(iri))
            .collect(),
        None => ctx
            .release
            .local_terms()
            .filter(|t| t.namespace == ns.iri)
            .collect(),
    };

    // How to use it. The register is deliberately prescriptive, following the
    // pattern the Wikidata MCP server uses with agents. The prefix
    // declarations come from the namespaces the listed terms are actually in,
    // which is not always this document's own namespace: a shapes document
    // names its shapes in the vocabulary namespace.
    let mut declarations: Vec<String> = Vec::new();
    let mut seen_namespaces: Vec<&str> = Vec::new();
    for t in &terms {
        if seen_namespaces.contains(&t.namespace.as_str()) {
            continue;
        }
        seen_namespaces.push(&t.namespace);
        let prefix = ctx
            .plan
            .namespace(&t.namespace)
            .and_then(|n| n.prefix.clone())
            .or_else(|| ns.prefix.clone())
            .unwrap_or_else(|| "ns".to_owned());
        declarations.push(format!("`@prefix {prefix}: <{}> .`", t.namespace));
    }
    if declarations.is_empty() {
        let prefix = ns.prefix.clone().unwrap_or_else(|| "ns".to_owned());
        declarations.push(format!("`@prefix {prefix}: <{}> .`", ns.iri));
    }
    let _ = writeln!(
        out,
        "Declare {} before using these terms. Every IRI listed below resolves; do not \
         invent term IRIs, and do not guess a local name that is not in this file. \
         Turtle is the canonical serialisation. Terms marked deprecated must not be \
         used in new data.\n",
        declarations.join(" and ")
    );

    if terms.len() > ctx.config.llms.max_terms {
        let _ = writeln!(
            out,
            "This vocabulary has {} terms, more than fit in one index. Use the term \
             index instead.\n",
            terms.len()
        );
        let _ = writeln!(out, "## Terms\n");
        let _ = writeln!(
            out,
            "- [terms.json]({}{}terms.json): every term with its label, definition and \
             representation URLs.\n",
            ctx.plan.base_url, ns.mount
        );
    } else {
        let mut kinds: Vec<TermKind> = terms.iter().map(|t| t.kind).collect();
        kinds.sort();
        kinds.dedup();
        for kind in kinds {
            let _ = writeln!(out, "## {}\n", kind.section());
            for t in terms.iter().filter(|t| t.kind == kind) {
                let home = ctx.namespace_of(t).unwrap_or(ns);
                let url = ctx.plan.term_url(home, &t.local_name, Rep::Markdown);
                let mut note = t.summary(lang).unwrap_or_default();
                if t.deprecated {
                    let replacement = t
                        .replaced_by
                        .first()
                        .map(|r| format!(", replaced by {}", ctx.short(r)))
                        .unwrap_or_default();
                    note = format!("(deprecated{replacement}) {note}");
                }
                let _ = writeln!(out, "- [{}]({url}): {note}", t.display(lang));
            }
            out.push('\n');
        }
    }

    if let Some(d) = doc
        && !d.foreign_terms.is_empty()
    {
        let _ = writeln!(out, "## Terms reused from other vocabularies\n");
        for iri in &d.foreign_terms {
            let Some(t) = ctx.release.term(iri) else {
                continue;
            };
            let note = t.summary(lang).unwrap_or_default();
            let _ = writeln!(out, "- `{}`: {note}", t.curie.as_deref().unwrap_or(&t.iri));
        }
        out.push('\n');
    }

    let _ = writeln!(out, "## Serialisations\n");
    let _ = writeln!(
        out,
        "- [Turtle]({}): the vocabulary graph, the canonical machine form.",
        ctx.plan.document_url(ns, Rep::Turtle)
    );
    let _ = writeln!(
        out,
        "- [Release Turtle]({}release.ttl): every document of this release unioned, \
         which is what a validator needs.",
        ctx.plan.base_url
    );
    let _ = writeln!(
        out,
        "- [JSON-LD]({}): the same graph as JSON, compacted with the context below.",
        ctx.plan.document_url(ns, Rep::JsonLd)
    );
    let _ = writeln!(
        out,
        "- [JSON-LD context]({}{}context.jsonld): maps each term to a short key, \
         so a JSON reader can use local names instead of IRIs.",
        ctx.plan.base_url, ns.mount
    );
    let _ = writeln!(
        out,
        "- [Term index]({}{}terms.json): the same terms as JSON.",
        ctx.plan.base_url, ns.mount
    );
    // A snapshot is the reason a citation keeps working, so the agent index
    // has to say it exists and where the list of them lives.
    let versions = ctx.snapshots(ns);
    if !versions.is_empty() {
        let _ = writeln!(
            out,
            "- [Version history]({}{}versions.ttl): which releases exist, as RDF; \
             also in {}versions.json.",
            ctx.plan.base_url, ns.mount, ctx.plan.base_url
        );
        for snap in &versions {
            let _ = writeln!(
                out,
                "- [Release {}]({}): this namespace as it stood at that release. \
                 It does not change.",
                snap.segment, snap.url
            );
        }
    }
    out.push('\n');

    let _ = writeln!(out, "## Optional\n");
    if ctx.changes.is_some_and(|d| {
        d.changes
            .iter()
            .any(|c| c.namespace.as_deref() == Some(ns.iri.as_str()))
    }) {
        let _ = writeln!(
            out,
            "- [Changes]({}{}changes.md): what is different from the previous release, \
             grouped by whether it breaks anything.",
            ctx.plan.base_url, ns.mount
        );
    }
    if ctx.config.site.pdf {
        let _ = writeln!(
            out,
            "- [PDF]({}{}{}.pdf): the same content as a tagged PDF/A document.",
            ctx.plan.base_url, ns.mount, ns.stem
        );
    }
    let _ = writeln!(
        out,
        "- [Human documentation]({}): the same content as HTML.",
        ctx.plan.document_url(ns, Rep::Html)
    );
    let _ = writeln!(
        out,
        "- [Markdown of this document]({}): the page an agent can read directly.",
        ctx.plan.document_url(ns, Rep::Markdown)
    );
    let _ = writeln!(
        out,
        "- [llms-full.txt]({}llms-full.txt): every term page concatenated.",
        ctx.plan.base_url
    );
    if let Some(data) = &ctx.config.llms.data_site {
        let _ = writeln!(
            out,
            "- [Companion dataset]({data}): data described with this vocabulary."
        );
    }

    out
}

/// The site-level index, which points at each namespace's own file. The
/// most-specific file wins, so an agent that lands here is told where to go.
pub fn root(ctx: &Ctx<'_>) -> String {
    let lang = ctx.lang();
    let mut out = String::new();
    let root_doc = ctx.release.root_document();
    let title = ctx
        .config
        .site
        .title
        .clone()
        .or_else(|| root_doc.map(|d| d.display(lang).to_owned()))
        .unwrap_or_else(|| "Vocabulary release".to_owned());
    let _ = writeln!(out, "# {title}\n");

    let summary = root_doc
        .and_then(|d| d.description(lang))
        .map(|d| d.value.replace(['\n', '\r'], " "))
        .unwrap_or_else(|| "A vocabulary release.".to_owned());
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = writeln!(
        out,
        "> {summary} The release has {} documents and {} terms.\n",
        ctx.release.stats.documents, ctx.release.stats.terms_local
    );

    let _ = writeln!(
        out,
        "Each namespace below has its own llms.txt with a line per term; the \
         most specific file applies. Every term also has a Markdown sibling at its \
         own URL.\n"
    );

    let _ = writeln!(out, "## Vocabularies\n");
    for ns in &ctx.plan.namespaces {
        let doc = ns
            .document
            .as_deref()
            .and_then(|iri| ctx.release.document(iri));
        let name = doc
            .map(|d| d.display(lang).to_owned())
            .unwrap_or_else(|| ns.iri.clone());
        let count = match doc {
            Some(d) => d.terms.len(),
            None => ctx
                .release
                .local_terms()
                .filter(|t| t.namespace == ns.iri)
                .count(),
        };
        let _ = writeln!(
            out,
            "- [{name}]({}): {count} terms in `{}`.",
            ctx.plan.llms_url(ns),
            ns.iri
        );
    }
    out.push('\n');

    let _ = writeln!(out, "## Serialisations\n");
    let _ = writeln!(
        out,
        "- [Release Turtle]({}release.ttl): every document unioned.",
        ctx.plan.base_url
    );
    let _ = writeln!(
        out,
        "- [Release JSON-LD]({}release.jsonld): the same graph as JSON.",
        ctx.plan.base_url
    );
    let _ = writeln!(
        out,
        "- [JSON-LD context]({}context.jsonld): every term of the release keyed by \
         its short name.",
        ctx.plan.base_url
    );
    let _ = writeln!(
        out,
        "- [Term index]({}terms.json): every term in the release.",
        ctx.plan.base_url
    );
    let _ = writeln!(
        out,
        "- [Manifest]({}manifest.json): namespaces, representations and how each \
         IRI resolves.\n",
        ctx.plan.base_url
    );

    let _ = writeln!(out, "## Optional\n");
    let _ = writeln!(
        out,
        "- [llms-full.txt]({}llms-full.txt): every term page concatenated.",
        ctx.plan.base_url
    );
    if let Some(data) = &ctx.config.llms.data_site {
        let _ = writeln!(out, "- [Companion dataset]({data}).");
    }

    out
}

/// Every per-term Markdown page concatenated. Not part of the llms.txt spec,
/// but a de facto companion that several documentation sites publish.
pub fn full(ctx: &Ctx<'_>) -> Result<String> {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} (full text)\n",
        ctx.config
            .site
            .title
            .clone()
            .or_else(|| ctx
                .release
                .root_document()
                .map(|d| d.display(ctx.lang()).to_owned()))
            .unwrap_or_else(|| "Vocabulary release".to_owned())
    );
    for ns in &ctx.plan.namespaces {
        for term in ctx.release.local_terms().filter(|t| t.namespace == ns.iri) {
            out.push_str("\n---\n\n");
            out.push_str(&super::markdown::term(ctx, ns, term)?);
        }
    }
    Ok(out)
}
