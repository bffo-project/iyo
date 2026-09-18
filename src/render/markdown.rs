//! Per-term and per-document Markdown, the form agents read.
//!
//! The shape is fixed by `docs/output-convention.md`: a blockquote
//! pointing at the covering `llms.txt`, then labels and definitions first
//! because they are what drives accuracy, then facts with the predicate each
//! came from, then an example, then the statements verbatim so nothing is
//! silently lost, then the sibling representations.

use super::{Ctx, humanise};
use crate::model::{Document, Term};
use crate::site::{NamespacePlan, Rep};
use anyhow::Result;
use std::fmt::Write;

fn fact(out: &mut String, name: &str, value: &str) {
    let _ = writeln!(out, "- {name}: {value}");
}

/// Which site generator's front matter to prefix per-term Markdown with
/// (`docs/output-convention.md`, "File layout per term"), so ODK/MkDocs,
/// Jekyll and Hugo treat each file as a titled page rather than untitled
/// body text.
///
/// All three carry `title`, `iri`, `kind` and `weight`. Hugo, Kramdown
/// (Jekyll) and MkDocs (via the `meta`/`mkdocs-material` YAML block) all
/// read the same `---`-fenced YAML block for a page's `title`, so there is
/// no framework-specific field among those four the spec's names leave out;
/// inventing one would be documenting an interface nobody asked for. The
/// variant is kept as a real, distinct flag anyway because it is the stated
/// interface (the convention names three targets, not one), and because a
/// difference any of them genuinely needs later has somewhere to live without
/// a breaking change to callers.
///
/// Hugo turned out to need exactly that difference, immediately: it derives
/// a page's URL from its source filename and *lowercases* that slug, which
/// silently folds `FormatVersion.md` and `formatVersion.md` onto the same
/// `/formatversion/` path — the very case-fold collision `NamespacePlan::cased`
/// exist to keep apart, reappearing in a consumer that
/// insists on doing its own path-casing. Hugo's front matter alone therefore
/// carries a fifth field, `url`, pinning the exact case-preserving request
/// path (`site::Plan::term_request_path`). MkDocs and Jekyll use the source
/// filename verbatim as the URL and have no such bug to work around, so they
/// do not get it. Do not "simplify" the three variants back into one shared
/// block: `url` on MkDocs or Jekyll would be a page-relative-path override
/// they do not need and did not ask for; its absence there is deliberate,
/// not an oversight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontMatter {
    /// No front matter. Byte-identical to the output before this flag
    /// existed.
    #[default]
    None,
    Hugo,
    MkDocs,
    Jekyll,
}

impl FrontMatter {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "hugo" => Some(Self::Hugo),
            "mkdocs" => Some(Self::MkDocs),
            "jekyll" => Some(Self::Jekyll),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hugo => "hugo",
            Self::MkDocs => "mkdocs",
            Self::Jekyll => "jekyll",
        }
    }
}

/// Quote and escape a scalar for a `key: "value"` YAML line.
///
/// Quoting unconditionally, rather than only when a special character
/// shows up, is what makes this correct: a label that starts with a YAML
/// indicator (`-`, `#`, `:`, `[`, `{`, `&`, `*`, `!`, `|`, `>`, `%`, `@`,
/// backtick, a quote) or reads as another type (`true`, `null`, `123`)
/// would otherwise change meaning or fail to parse. Double-quoted YAML
/// scalars support backslash escapes, so backslash and the closing quote
/// are the only bytes that must be escaped; control characters are escaped
/// too so a stray newline in a label can never break the line structure of
/// the block.
fn yaml_scalar(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Front matter for a per-term page: `title`, `iri`, `kind`, `weight`,
/// plus `url` for `FrontMatter::Hugo` alone (see the module doc
/// comment), or the empty string for `FrontMatter::None` so a `none` build
/// stays byte-identical to a build with no `--md-frontmatter` flag at all.
///
/// `title` and `kind` are passed in by the caller rather than recomputed
/// here so they cannot drift from the `# ` heading and the `- Kind:` fact
/// this same page's body already renders them as.
///
/// `weight` is the term's 1-based position among the local terms of its own
/// namespace, in the order `Release::local_terms` yields them: alphabetical
/// by IRI, which is also the order the build writes term files in. It is a
/// stable ordering carried over from the model, not a ranking this function
/// invents.
///
/// `url` is always passed in, even for the three styles that ignore it,
/// rather than requiring three call sites to know which ones care: it is a
/// cheap `format!` (`site::Plan::term_request_path`), and computing it
/// unconditionally is simpler than threading `Option` through every caller
/// for one style's benefit.
pub fn term_front_matter(
    style: FrontMatter,
    title: &str,
    iri: &str,
    kind: &str,
    weight: usize,
    url: &str,
) -> String {
    if style == FrontMatter::None {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "title: {}", yaml_scalar(title));
    let _ = writeln!(out, "iri: {}", yaml_scalar(iri));
    let _ = writeln!(out, "kind: {}", yaml_scalar(kind));
    let _ = writeln!(out, "weight: {weight}");
    if style == FrontMatter::Hugo {
        let _ = writeln!(out, "url: {}", yaml_scalar(url));
    }
    out.push_str("---\n\n");
    out
}

/// Rewrite a document's link targets relative to the file it is written at.
///
/// These pages already distinguish the two jobs a URL does, by how it is
/// written. A bare URL states where something *is*: the term's IRI, its
/// canonical page, the release that defines it, the other representations. It
/// stays absolute, so a file read on its own, detached from any origin, still
/// says what it is and where it came from. A `](target)` is something the
/// reader follows, and becomes relative, so that one build is navigable from
/// the origin it names, from a preview, and from a checkout on disk.
///
/// Every link target a build emits is an internal cross-reference to another
/// `.md`; the check below fails the build if that ever stops being true.
pub fn relativise_links(body: &str, from: &str, base_url: &str) -> String {
    if base_url.is_empty() {
        return body.to_owned();
    }
    let from = format!("/{}", from.trim_start_matches('/'));
    let needle = format!("]({base_url}");
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(&needle) {
        out.push_str(&rest[..at]);
        out.push_str("](");
        rest = &rest[at + needle.len()..];
        let Some(close) = rest.find(')') else {
            // An unterminated link is not a link. Leave it exactly as it was.
            out.push_str(base_url);
            continue;
        };
        let target = format!("/{}", &rest[..close]);
        out.push_str(&crate::negotiate::relative_to(&from, &target));
        out.push(')');
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

fn list(ctx: &Ctx<'_>, ns: &NamespacePlan, iris: &[String]) -> String {
    iris.iter()
        .map(|i| ctx.link_md(i, ns))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A shape's fields as a Markdown table.
///
/// The convention asks for facts that name their source, so every
/// constraint here says which shape imposes it. A reader must be able to tell
/// "the vocabulary says the range is a concept" from "this shape says the
/// value must come from that scheme": both are true and only one is a
/// validation rule.
fn shape_table(ctx: &Ctx<'_>, ns: &NamespacePlan, shape: &crate::shape::NodeShape) -> String {
    let mut out = String::new();
    // A column of its own for the description, unlike the HTML table, which
    // puts it under the field name: prose in a narrow cell is unreadable on a
    // page and perfectly readable in a file an agent parses.
    let _ = writeln!(out, "| Field | Property | Values | Count | Description |");
    let _ = writeln!(out, "| --- | --- | --- | --- | --- |");
    for p in &shape.properties {
        let name = p.name.clone().unwrap_or_else(|| path_text(ctx, &p.path));
        let property = match p.path.as_ref().and_then(crate::shape::Path::predicate) {
            Some(iri) => ctx.link_md(iri, ns),
            None => format!("`{}`", path_text(ctx, &p.path)),
        };
        let mut values: Vec<String> = Vec::new();
        let types: Vec<String> = p
            .datatypes
            .iter()
            .chain(p.classes.iter())
            .map(|d| ctx.short(d))
            .collect();
        if !types.is_empty() {
            values.push(types.join(" or "));
        } else if let Some(kind) = &p.node_kind {
            values.push(crate::vocab::local_name(kind).to_owned());
        }
        for scheme in &p.in_scheme {
            values.push(format!("from {}", ctx.link_md(scheme, ns)));
        }
        if !p.values.is_empty() {
            values.push(format!("one of {}", p.values.join(", ")));
        }
        if let Some(pattern) = &p.pattern {
            values.push(format!("matching `{pattern}`"));
        }
        let count = match p.cardinality() {
            Some(c) if p.required() => format!("`{c}` required"),
            Some(c) => format!("`{c}`"),
            None => "unconstrained".to_owned(),
        };
        // A pipe inside a cell would end it early and shift every column.
        let description = p
            .description
            .as_deref()
            .unwrap_or_default()
            .replace('|', "\\|");
        let _ = writeln!(
            out,
            "| {name} | {property} | {} | {count} | {description} |",
            values.join("; ")
        );
    }
    out.push('\n');
    out
}

fn path_text(ctx: &Ctx<'_>, path: &Option<crate::shape::Path>) -> String {
    match path {
        Some(crate::shape::Path::Predicate { iri }) => ctx.short(iri),
        Some(crate::shape::Path::Inverse { iri }) => format!("inverse of {}", ctx.short(iri)),
        Some(crate::shape::Path::Complex { description }) => description.clone(),
        None => String::new(),
    }
}

/// The Markdown page of one term.
pub fn term(ctx: &Ctx<'_>, ns: &NamespacePlan, term: &Term) -> Result<String> {
    let lang = ctx.lang();
    let mut out = String::new();
    let document = ctx.document_of(term);

    let index = ctx.plan.llms_url(ns);
    let canonical = ctx.plan.term_url(ns, &term.local_name, Rep::Html);
    match document {
        Some(doc) => {
            let _ = writeln!(
                out,
                "> Part of {}. Index: {index}. Canonical page: {canonical}",
                doc.display(lang)
            );
        }
        None => {
            let _ = writeln!(out, "> Index: {index}. Canonical page: {canonical}");
        }
    }
    out.push('\n');

    match &term.curie {
        Some(c) => {
            let _ = writeln!(out, "# {} ({c})", term.display(lang));
        }
        None => {
            let _ = writeln!(out, "# {}", term.display(lang));
        }
    }
    out.push('\n');

    if term.deprecated {
        let replacement = if term.replaced_by.is_empty() {
            "no replacement is recorded".to_owned()
        } else {
            format!("use {}", list(ctx, ns, &term.replaced_by))
        };
        let _ = writeln!(out, "**Deprecated**: {replacement}.\n");
    }

    fact(&mut out, "IRI", &format!("`{}`", term.iri));
    fact(&mut out, "Kind", term.kind.label());
    if let Some(doc) = document {
        let mut where_ = doc.iri.to_string();
        let mut extra = Vec::new();
        if let Some(v) = &doc.header.version_info {
            extra.push(format!("version {v}"));
        }
        if let Some(s) = &doc.header.status {
            extra.push(humanise(s));
        }
        if !extra.is_empty() {
            where_ = format!("{where_} ({})", extra.join(", "));
        }
        fact(&mut out, "Defined by", &where_);
    }
    for label in &term.labels {
        let tag = label
            .lang
            .as_deref()
            .map(|l| format!(" [{l}]"))
            .unwrap_or_default();
        fact(
            &mut out,
            "Label",
            &format!(
                "{}{tag} (source: {})",
                label.value,
                ctx.short(&label.source)
            ),
        );
    }
    for alt in &term.alt_labels {
        fact(&mut out, "Alternative label", &alt.value);
    }
    if let Some(n) = &term.concept.notation {
        fact(&mut out, "Notation", &format!("`{n}`"));
    }
    if !term.super_terms.is_empty() {
        let name = match term.kind {
            crate::model::TermKind::Class => "Subclass of",
            _ => "Sub-property of",
        };
        fact(&mut out, name, &list(ctx, ns, &term.super_terms));
    }
    if !term.sub_terms.is_empty() {
        fact(&mut out, "Narrower terms", &list(ctx, ns, &term.sub_terms));
    }
    if !term.equivalent.is_empty() {
        fact(&mut out, "Equivalent to", &list(ctx, ns, &term.equivalent));
    }
    if !term.disjoint_with.is_empty() {
        fact(
            &mut out,
            "Disjoint with",
            &list(ctx, ns, &term.disjoint_with),
        );
    }
    if !term.property.domain.is_empty() {
        fact(&mut out, "Domain", &list(ctx, ns, &term.property.domain));
    }
    if !term.property.range.is_empty() {
        fact(&mut out, "Range", &list(ctx, ns, &term.property.range));
    }
    if !term.property.domain_includes.is_empty() {
        fact(
            &mut out,
            "Domain includes",
            &list(ctx, ns, &term.property.domain_includes),
        );
    }
    if !term.property.range_includes.is_empty() {
        fact(
            &mut out,
            "Range includes",
            &list(ctx, ns, &term.property.range_includes),
        );
    }
    if !term.property.inverse_of.is_empty() {
        fact(
            &mut out,
            "Inverse of",
            &list(ctx, ns, &term.property.inverse_of),
        );
    }
    if !term.property.characteristics.is_empty() {
        fact(
            &mut out,
            "Characteristics",
            &term.property.characteristics.join(", "),
        );
    }
    if !term.concept.in_scheme.is_empty() {
        fact(
            &mut out,
            "In scheme",
            &list(ctx, ns, &term.concept.in_scheme),
        );
    }
    if !term.concept.top_concept_of.is_empty() {
        fact(
            &mut out,
            "Top concept of",
            &list(ctx, ns, &term.concept.top_concept_of),
        );
    }
    if !term.concept.broader.is_empty() {
        fact(&mut out, "Broader", &list(ctx, ns, &term.concept.broader));
    }
    if !term.concept.narrower.is_empty() {
        fact(&mut out, "Narrower", &list(ctx, ns, &term.concept.narrower));
    }
    if !term.concept.related.is_empty() {
        fact(&mut out, "Related", &list(ctx, ns, &term.concept.related));
    }
    for m in &term.mappings {
        fact(
            &mut out,
            &humanise(&m.relation),
            &format!("{} (`{}`)", ctx.short(&m.iri), m.iri),
        );
    }
    if let Some(s) = &term.status {
        fact(&mut out, "Status", &humanise(s));
    }
    for s in &term.see_also {
        fact(&mut out, "See also", &format!("<{s}>"));
    }

    // What shapes say about this property, kept separate from the facts above
    // so that OWL and SHACL are never merged into one unattributed claim.
    for (shape, p) in ctx.release.shapes.for_property(&term.iri) {
        let source = format!(" (source: SHACL {})", ctx.short(&shape.iri));
        if let Some(name) = &p.name {
            fact(&mut out, "Field name", &format!("{name}{source}"));
        }
        if let Some(c) = p.cardinality() {
            let need = if p.required() { "required" } else { "optional" };
            fact(&mut out, "Cardinality", &format!("{c}, {need}{source}"));
        }
        for scheme in &p.in_scheme {
            fact(
                &mut out,
                "Values from",
                &format!("{}{source}", ctx.link_md(scheme, ns)),
            );
        }
        if !p.values.is_empty() {
            fact(
                &mut out,
                "One of",
                &format!("{}{source}", p.values.join(", ")),
            );
        }
    }

    if !term.definitions.is_empty() {
        out.push_str("\n## Definition\n\n");
        for d in &term.definitions {
            let _ = writeln!(out, "{}\n", d.value);
            let _ = writeln!(out, "*Source: {}*\n", ctx.short(&d.source));
        }
    }
    if !term.comments.is_empty() {
        out.push_str("## Comment\n\n");
        for c in &term.comments {
            let _ = writeln!(out, "{}\n", c.value);
        }
    }
    if !term.notes.is_empty() {
        out.push_str("## Notes\n\n");
        for n in &term.notes {
            let _ = writeln!(out, "- {}", n.value);
        }
        out.push('\n');
    }
    if !term.examples.is_empty() {
        out.push_str("## Examples\n\n");
        for e in &term.examples {
            let _ = writeln!(out, "```turtle\n{}\n```\n", e.value.trim());
        }
    }

    let own = ctx.release.shapes.shape(&term.iri).into_iter();
    let for_class = ctx
        .release
        .shapes
        .for_class(&term.iri)
        .into_iter()
        .filter(|s| s.iri != term.iri);
    for shape in own.chain(for_class) {
        let heading = if shape.iri == term.iri {
            "## Constraints".to_owned()
        } else {
            format!("## Record template from {}", ctx.short(&shape.iri))
        };
        let _ = writeln!(out, "{heading}\n");
        let required = shape.properties.iter().filter(|p| p.required()).count();
        let _ = writeln!(
            out,
            "{} field{}, {required} required.\n",
            shape.properties.len(),
            if shape.properties.len() == 1 { "" } else { "s" }
        );
        out.push_str(&shape_table(ctx, ns, shape));
    }

    let turtle = super::rdf::term(ctx.store, &term.iri, &ctx.release.prefixes)?;
    out.push_str("## Statements\n\n```turtle\n");
    out.push_str(turtle.trim_end());
    out.push_str("\n```\n\n");

    out.push_str("## Also available as\n\n");
    for rep in Rep::produced() {
        let _ = writeln!(
            out,
            "- {}: {}",
            match rep {
                Rep::Html => "HTML",
                Rep::Turtle => "Turtle",
                Rep::JsonLd => "JSON-LD",
                Rep::Markdown => "Markdown",
            },
            ctx.plan.term_url(ns, &term.local_name, rep)
        );
    }

    Ok(out)
}

/// The Markdown page of a namespace document: metadata, then a term reference.
///
/// `--md-frontmatter` does not touch this page. The spec names
/// "per-term Markdown" only, and unlike a term this page's body has no
/// `- Kind:` fact to reuse for a `kind` value, so adding one here would be
/// inventing a field the spec never named rather than reusing an existing
/// fact. The `# ` heading below still gives Hugo/MkDocs/Jekyll a title to
/// fall back on for this file.
pub fn document(ctx: &Ctx<'_>, ns: &NamespacePlan, doc: &Document) -> String {
    let lang = ctx.lang();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "> Vocabulary document. Index: {}. Canonical page: {}",
        ctx.plan.llms_url(ns),
        ctx.plan.document_url(ns, Rep::Html)
    );
    out.push('\n');
    let _ = writeln!(out, "# {}\n", doc.display(lang));

    if let Some(status) = &doc.header.status {
        let _ = writeln!(
            out,
            "**Status**: {}{}.\n",
            humanise(status),
            doc.header
                .version_info
                .as_ref()
                .map(|v| format!(", version {v}"))
                .unwrap_or_default()
        );
    }

    if let Some(d) = doc.description(lang) {
        let _ = writeln!(out, "{}\n", d.value);
    }
    for a in &doc.header.abstract_ {
        let _ = writeln!(out, "{}\n", a.value);
    }

    out.push_str("## Metadata\n\n");
    fact(&mut out, "Namespace IRI", &format!("`{}`", ns.iri));
    if let Some(p) = &ns.prefix {
        fact(&mut out, "Preferred prefix", &format!("`{p}`"));
    }
    if let Some(v) = &doc.header.version_iri {
        fact(&mut out, "This version", v);
    }
    if let Some(v) = &doc.header.version_info {
        fact(&mut out, "Version", v);
    }
    for c in &doc.header.creators {
        if let Some(name) = &c.name {
            fact(&mut out, "Creator", name);
        }
    }
    for p in &doc.header.publishers {
        if let Some(name) = &p.name {
            fact(&mut out, "Publisher", name);
        }
    }
    if let Some(d) = &doc.header.created {
        fact(&mut out, "Created", d);
    }
    if let Some(d) = &doc.header.modified {
        fact(&mut out, "Modified", d);
    }
    if let Some(l) = &doc.header.license {
        fact(&mut out, "Licence of the vocabulary", &format!("<{l}>"));
    }
    if let Some(l) = &ctx.config.site.doc_license {
        fact(&mut out, "Licence of this documentation", &format!("<{l}>"));
    }
    if !doc.header.has_part.is_empty() {
        fact(&mut out, "Parts", &list(ctx, ns, &doc.header.has_part));
    }
    for s in &doc.header.see_also {
        fact(&mut out, "See also", &format!("<{s}>"));
    }
    out.push('\n');

    // Term reference, grouped by kind in a stable order.
    let mut kinds: Vec<crate::model::TermKind> = doc
        .terms
        .iter()
        .filter_map(|iri| ctx.release.term(iri).map(|t| t.kind))
        .collect();
    kinds.sort();
    kinds.dedup();
    for kind in kinds {
        let _ = writeln!(out, "## {}\n", kind.section());
        for iri in &doc.terms {
            let Some(t) = ctx.release.term(iri) else {
                continue;
            };
            if t.kind != kind {
                continue;
            }
            let Some(term_ns) = ctx.namespace_of(t) else {
                continue;
            };
            let url = ctx.plan.term_url(term_ns, &t.local_name, Rep::Markdown);
            let summary = t.summary(lang).unwrap_or_default();
            let _ = writeln!(out, "- [{}]({url}): {summary}", t.display(lang));
        }
        out.push('\n');
    }

    if !doc.foreign_terms.is_empty() {
        out.push_str("## Terms reused from other vocabularies\n\n");
        for iri in &doc.foreign_terms {
            let Some(t) = ctx.release.term(iri) else {
                continue;
            };
            let note = t
                .summary(lang)
                .or_else(|| t.notes.first().map(|n| n.value.clone()))
                .unwrap_or_default();
            let _ = writeln!(out, "- `{}` (`{}`): {note}", t.display(lang), t.iri);
            // A reused term has no page of its own, so a shape's requirement
            // on it can only be stated here, and stated as the shape's claim
            // rather than the vocabulary's.
            for (shape, p) in ctx.release.shapes.for_property(&t.iri) {
                // "constrained by", not "required by": a shape that says
                // nothing about counts requires nothing.
                let mut parts = vec![format!("constrained by {}", ctx.short(&shape.iri))];
                if let Some(name) = &p.name {
                    parts.push(format!("as \"{name}\""));
                }
                if let Some(c) = p.cardinality() {
                    parts.push(if p.required() {
                        format!("{c} required")
                    } else {
                        c
                    });
                }
                if let Some(d) = &p.description {
                    parts.push(d.clone());
                }
                let _ = writeln!(out, "  - {}", parts.join("; "));
            }
        }
        out.push('\n');
    }

    out.push_str("## Also available as\n\n");
    let _ = writeln!(out, "- HTML: {}", ctx.plan.document_url(ns, Rep::Html));
    let _ = writeln!(out, "- Turtle: {}", ctx.plan.document_url(ns, Rep::Turtle));
    let _ = writeln!(out, "- JSON-LD: {}", ctx.plan.document_url(ns, Rep::JsonLd));
    let _ = writeln!(
        out,
        "- JSON-LD context: {}{}context.jsonld",
        ctx.plan.base_url, ns.mount
    );
    let _ = writeln!(out, "- Agent index: {}", ctx.plan.llms_url(ns));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_produces_no_front_matter() {
        assert_eq!(
            term_front_matter(
                FrontMatter::None,
                "Format",
                "https://example.org/Format",
                "class",
                1,
                "/Format",
            ),
            ""
        );
    }

    #[test]
    fn each_variant_is_delimited_and_carries_all_four_shared_fields() {
        for style in [FrontMatter::Hugo, FrontMatter::MkDocs, FrontMatter::Jekyll] {
            let fm = term_front_matter(
                style,
                "Format",
                "https://example.org/Format",
                "class",
                3,
                "/Format",
            );
            assert!(
                fm.starts_with("---\n"),
                "{}: missing opening fence",
                style.as_str()
            );
            // The blank line the caller relies on to separate front matter
            // from the blockquote that already starts the body.
            assert!(
                fm.ends_with("---\n\n"),
                "{}: missing closing fence and blank line",
                style.as_str()
            );
            for field in ["title:", "iri:", "kind:", "weight:"] {
                assert!(
                    fm.contains(field),
                    "{}: missing field {field}",
                    style.as_str()
                );
            }
            assert!(fm.contains("title: \"Format\""));
            assert!(fm.contains("iri: \"https://example.org/Format\""));
            assert!(fm.contains("kind: \"class\""));
            assert!(fm.contains("weight: 3"));
        }
    }

    /// MkDocs and Jekyll are, deliberately, the same YAML block: the
    /// convention names four fields for all three and no consumer-specific
    /// field was found for either (see the module doc comment on
    /// `FrontMatter`). Hugo is deliberately not part of that equality any
    /// more: it alone gets a fifth field, `url`, because it alone lowercases a
    /// page's URL from its filename (see the same doc comment). This test
    /// would fail the moment someone "simplified" the three back into one
    /// shared block.
    #[test]
    fn only_mkdocs_and_jekyll_render_identical_front_matter() {
        let hugo = term_front_matter(
            FrontMatter::Hugo,
            "Format",
            "https://example.org/Format",
            "class",
            1,
            "/Format",
        );
        let mkdocs = term_front_matter(
            FrontMatter::MkDocs,
            "Format",
            "https://example.org/Format",
            "class",
            1,
            "/Format",
        );
        let jekyll = term_front_matter(
            FrontMatter::Jekyll,
            "Format",
            "https://example.org/Format",
            "class",
            1,
            "/Format",
        );
        assert_eq!(mkdocs, jekyll);
        assert_ne!(hugo, mkdocs);
        // The only difference is the added `url:` line.
        assert_eq!(
            hugo,
            format!("{}url: \"/Format\"\n---\n\n", &mkdocs[..mkdocs.len() - 5])
        );
    }

    #[test]
    fn only_hugo_gets_a_url_field() {
        for style in [FrontMatter::MkDocs, FrontMatter::Jekyll] {
            let fm = term_front_matter(
                style,
                "Format",
                "https://example.org/Format",
                "class",
                1,
                "/Format",
            );
            assert!(
                !fm.contains("url:"),
                "{}: should not carry url",
                style.as_str()
            );
        }
        let hugo = term_front_matter(
            FrontMatter::Hugo,
            "Format",
            "https://example.org/Format",
            "class",
            1,
            "/Format",
        );
        assert!(hugo.contains("url: \"/Format\""));
    }

    #[test]
    fn yaml_hostile_labels_are_quoted_and_escaped() {
        // A colon-plus-space would otherwise end the key early; a leading
        // `#` would otherwise start a comment; a `"` must not close the
        // quote early; a backslash must not escape the following quote.
        let label = "Weird: \"quoted\" # not a comment \\ backslash";
        let fm = term_front_matter(
            FrontMatter::Hugo,
            label,
            "https://example.org/Weird",
            "class",
            1,
            "/Weird",
        );
        let title_line = fm
            .lines()
            .find(|l| l.starts_with("title:"))
            .expect("a title line");
        assert_eq!(
            title_line,
            r#"title: "Weird: \"quoted\" # not a comment \\ backslash""#
        );
    }

    #[test]
    fn a_hugo_url_hostile_target_is_quoted_and_escaped_too() {
        let fm = term_front_matter(
            FrontMatter::Hugo,
            "Format",
            "https://example.org/Format",
            "class",
            1,
            "/Weird: \"quoted\"",
        );
        let url_line = fm
            .lines()
            .find(|l| l.starts_with("url:"))
            .expect("a url line");
        assert_eq!(url_line, r#"url: "/Weird: \"quoted\"""#);
    }

    #[test]
    fn yaml_scalar_escapes_control_characters() {
        assert_eq!(yaml_scalar("a\nb\tc\rd"), r#""a\nb\tc\rd""#);
    }
}
