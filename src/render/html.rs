//! HTML pages.
//!
//! The default theme is embedded in the binary and a `--theme DIR` overlays it
//! file by file, so overriding the footer means writing one file rather than
//! forking (`docs/theming.md`, "The override surface"). Templates receive the
//! view model of `view.rs` and a small filter set; they never see RDF.
//!
//! Two things are done in Rust rather than in a template on purpose. URLs are
//! marked safe after validation, because minijinja's HTML autoescape also
//! escapes `/` and would turn every link into `https:&#x2f;&#x2f;…`. The
//! JSON-LD block is serialised with `serde_json`, because escaping HTML inside
//! a JSON string corrupts it.

use super::{Ctx, view};
use crate::site::Rep;
use crate::theme::Tokens;
use anyhow::{Context as _, Result};
use camino::Utf8PathBuf;
use minijinja::{Environment, Error, ErrorKind, State, Value, path_loader};
use rust_embed::Embed;
use serde::Serialize;
use serde_json::json;

/// The default theme, compiled into the binary. `debug-embed` is on so that a
/// debug build reads the same bytes a release build does, which the
/// determinism hash depends on.
#[derive(Embed)]
#[folder = "assets/"]
struct DefaultTheme;

/// A template name and its source, for `theme check` and for the loader.
pub fn default_template(name: &str) -> Option<String> {
    DefaultTheme::get(&format!("templates/{name}"))
        .and_then(|f| String::from_utf8(f.data.into_owned()).ok())
}

/// Every asset that is not a template, for copying into the output.
pub fn default_assets() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = DefaultTheme::iter()
        .filter(|p| !p.starts_with("templates/") && p.ends_with(".css"))
        .filter_map(|p| {
            let file = DefaultTheme::get(&p)?;
            let text = String::from_utf8(file.data.into_owned()).ok()?;
            Some((p.to_string(), text))
        })
        .collect();
    // rust-embed iterates per directory; sort so the output is stable.
    out.sort();
    out
}

/// Reject anything that could break out of an attribute, then mark it safe.
///
/// IRIs cannot contain a double quote, a space or an angle bracket, so a value
/// that does is not a URL from the graph and is escaped normally.
fn url_filter(value: &str) -> Value {
    if value.contains(['"', '<', '>', ' ', '\n']) {
        Value::from(value)
    } else {
        Value::from_safe_string(value.to_owned())
    }
}

/// A navigational href, rewritten relative to the page it appears on.
///
/// Identity and navigation are two different jobs that happened to produce the
/// same string. `rel="canonical"`, `rel="cite-as"` and the RDF name the
/// resource and stay absolute. Anything a reader or a crawler follows goes
/// through here, so that one build serves from the origin it names, from a
/// local preview, and from a project page mounted in a subdirectory, without
/// being rebuilt for each. A build that has to be rebuilt per environment is
/// not the build that was tested.
///
/// A URL outside the site passes through untouched, which is what keeps
/// licence links and the generator link absolute.
fn rel_filter(state: &State, value: &str) -> Value {
    let base = lookup_str(state, "site", "base_url");
    let root = lookup_str(state, "page", "root");
    url_filter(&relative_href(value, &base, &root))
}

/// An absolute URL inside the site, rewritten relative to a page whose path
/// back to the site root is `root`. Anything outside the site, and anything at
/// all when there is no base URL to recognise, is returned unchanged.
pub fn relative_href(value: &str, base_url: &str, root: &str) -> String {
    if base_url.is_empty() {
        return value.to_owned();
    }
    match value.strip_prefix(base_url) {
        Some(rest) => {
            let href = format!("{root}{}", rest.trim_start_matches('/'));
            if href.is_empty() {
                "./".to_owned()
            } else {
                href
            }
        }
        None => value.to_owned(),
    }
}

/// One string attribute of one template variable, or empty when the template
/// was rendered without it.
fn lookup_str(state: &State, object: &str, attr: &str) -> String {
    state
        .lookup(object)
        .and_then(|v| v.get_attr(attr).ok())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// ` lang="ja"` when a literal's language differs from the page, else nothing.
fn lang_attr(value: Option<String>) -> Value {
    match value {
        Some(tag)
            if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
        {
            Value::from_safe_string(format!(" lang=\"{tag}\""))
        }
        _ => Value::from_safe_string(String::new()),
    }
}

/// Collapse a literal to one line, for a meta description or an index entry.
fn oneline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Copy a theme's `assets/` into the output, replacing embedded files of the
/// same name.
///
/// Text and binary are told apart by whether the bytes are UTF-8, not by
/// extension: a theme's `logo.svg` is text and its `fonts/Inter.woff2` is
/// not, and the auditor has something to say about the first and nothing
/// about the second. Guessing from the extension would put a UTF-16 SVG in
/// the wrong bucket and an ASCII `.bin` in the other.
///
/// `tokens.css` is refused rather than silently ignored: it is generated
/// from `tokens.toml` so that the custom properties and the contrast-gated
/// values cannot drift, and a theme shipping its own copy would be
/// overwritten on the next build with no sign of why.
fn theme_assets(theme_dir: Option<&camino::Utf8Path>, out: &mut super::Output) -> Result<()> {
    let Some(dir) = theme_dir.map(|d| d.join("assets")).filter(|d| d.is_dir()) else {
        return Ok(());
    };
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("reading the theme's assets in {current}"))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {current}"))?;
            let path = camino::Utf8PathBuf::from_path_buf(entry.path())
                .map_err(|p| anyhow::anyhow!("theme asset path is not UTF-8: {}", p.display()))?;
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&dir)
                .with_context(|| format!("{path} is not under {dir}"))?;
            if relative.as_str() == "tokens.css" {
                anyhow::bail!(
                    "{path} would be overwritten: tokens.css is generated from tokens.toml                      so the custom properties and the contrast-checked values cannot drift.                      Put the values in the theme's tokens.toml instead."
                );
            }
            let bytes = std::fs::read(&path).with_context(|| format!("reading {path}"))?;
            let site_path = format!("assets/{relative}");
            match String::from_utf8(bytes) {
                Ok(text) => out.add(site_path, text),
                Err(e) => out.add_bytes(site_path, e.into_bytes()),
            }
        }
    }
    Ok(())
}

/// Build the template environment: embedded defaults, overlaid by a theme dir.
pub fn environment(theme_dir: Option<Utf8PathBuf>) -> Environment<'static> {
    let user = theme_dir.map(|d| path_loader(d.join("templates").into_std_path_buf()));
    let mut env = Environment::new();
    env.set_loader(move |name: &str| -> Result<Option<String>, Error> {
        if let Some(loader) = &user
            && let Some(source) = loader(name)?
        {
            return Ok(Some(source));
        }
        match DefaultTheme::get(&format!("templates/{name}")) {
            Some(file) => match String::from_utf8(file.data.into_owned()) {
                Ok(text) => Ok(Some(text)),
                Err(_) => Err(Error::new(
                    ErrorKind::InvalidOperation,
                    format!("embedded template {name} is not UTF-8"),
                )),
            },
            None => Ok(None),
        }
    });
    // Environment::new() takes debug from cfg!(debug_assertions), so a release
    // build would report a theme error with no line and no excerpt.
    env.set_debug(true);
    env.set_keep_trailing_newline(true);
    env.add_filter("url", url_filter);
    env.add_filter("rel", rel_filter);
    env.add_filter("lang_attr", lang_attr);
    env.add_filter("oneline", oneline);
    env
}

/// The status banner, when the document declares one.
#[derive(Debug, Clone, Serialize)]
pub struct Banner {
    pub title: String,
    pub paragraphs: Vec<String>,
}

/// Per-page values that are not part of the vocabulary model.
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub kind: &'static str,
    /// The `<title>`, built to a length budget rather than concatenated.
    pub title: String,
    pub assets: String,
    /// The path from this page back to the site root, so that `rel` can turn
    /// an absolute URL from the model into an href that resolves wherever the
    /// tree is mounted.
    pub root: String,
    pub llms_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_url: Option<String>,
    /// Whether the identity IRI resolves back to this page. Signposting makes
    /// `cite-as` a conformance failure when it does not, so a deployment that
    /// cannot negotiate must not claim it.
    pub cite_as: bool,
    pub jsonld: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<Banner>,
    pub hierarchy_html: String,
}

/// The path from a page back to the site root, so that stylesheets resolve
/// wherever the tree is mounted and a build can be previewed from disk.
///
/// Absolute asset URLs would tie the output to one deployment and leave a
/// local preview unstyled, which is not a cosmetic problem: without the
/// stylesheet every list link falls below the 24px target size.
fn relative_root(page_path: &str) -> String {
    let depth = page_path.matches('/').count();
    if depth == 0 {
        String::new()
    } else {
        "../".repeat(depth)
    }
}

/// Longest useful title that stays inside the 70-character budget: the label,
/// then the CURIE, then the site name, each dropped only if it does not fit.
fn page_title(label: &str, curie: Option<&str>, site: &str) -> String {
    const BUDGET: usize = 70;
    let with_curie = match curie {
        Some(c) => format!("{label} ({c})"),
        None => label.to_owned(),
    };
    let head = if with_curie.chars().count() <= BUDGET {
        with_curie
    } else {
        label.to_owned()
    };
    let full = format!("{head} — {site}");
    if full.chars().count() <= BUDGET {
        full
    } else if head.chars().count() <= BUDGET {
        head
    } else {
        head.chars()
            .take(BUDGET - 1)
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…"
    }
}

pub fn licence_label(iri: &str) -> String {
    let tail = iri.trim_end_matches('/');
    let name = crate::vocab::local_name(tail);
    match name {
        "zero" | "1.0" if iri.contains("publicdomain/zero") => "CC0 1.0".to_owned(),
        _ => {
            if let Some(rest) = iri.strip_prefix("https://creativecommons.org/licenses/") {
                let mut parts = rest.trim_end_matches('/').split('/');
                let code = parts.next().unwrap_or("").to_uppercase();
                let version = parts.next().unwrap_or("");
                format!("CC {code} {version}").trim().to_owned()
            } else {
                iri.to_owned()
            }
        }
    }
}

fn banner_for(document: Option<&crate::model::Document>, lang: &str) -> Option<Banner> {
    let doc = document?;
    let status = doc.header.status.as_deref()?;
    let title = match &doc.header.version_info {
        Some(v) => format!("{}, version {v}", super::humanise(status)),
        None => super::humanise(status),
    };
    let paragraphs = doc
        .header
        .comment
        .iter()
        .filter(|c| c.lang.as_deref() == Some(lang) || c.lang.is_none())
        .map(|c| c.value.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    Some(Banner {
        title: title
            .char_indices()
            .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
            .collect(),
        paragraphs,
    })
}

/// The hierarchy is assembled here rather than in the template, so it does not
/// pass through the `rel` filter and has to relativise its own hrefs.
fn hierarchy_html(nodes: &[view::TreeNode], base_url: &str, root: &str) -> String {
    fn walk(nodes: &[view::TreeNode], base_url: &str, root: &str, out: &mut String) {
        out.push_str("<ul class=\"tree\">");
        for n in nodes {
            out.push_str("<li>");
            match &n.term.url {
                Some(url) => out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    relative_href(url, base_url, root),
                    html_escape(&n.term.label)
                )),
                None => out.push_str(&html_escape(&n.term.label)),
            }
            if !n.children.is_empty() {
                walk(&n.children, base_url, root, out);
            }
            out.push_str("</li>");
        }
        out.push_str("</ul>");
    }
    if nodes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    walk(nodes, base_url, root, &mut out);
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The JSON-LD block for a term: a page node joined to the term it is about.
///
/// `DefinedTerm` is a subclass of `Intangible`, so licence and publisher
/// belong on the `WebPage`, not on the term. `termCode` is the local name and
/// `@id` the identity IRI.
fn term_jsonld(ctx: &Ctx<'_>, t: &view::TermView, doc_iri: Option<&str>) -> String {
    let mut term = json!({
        "@type": "DefinedTerm",
        "@id": t.iri,
        "name": t.label,
        "termCode": t.local_name,
        "url": t.url,
    });
    if let Some(d) = &t.definition {
        term["description"] = json!(oneline(&d.value));
    }
    if let Some(iri) = doc_iri {
        term["inDefinedTermSet"] = json!(iri);
    }
    if !t.types_for_jsonld().is_empty() {
        term["additionalType"] = json!(t.types_for_jsonld());
    }
    let page = json!({
        "@type": "WebPage",
        "url": t.url,
        "name": t.label,
        "inLanguage": ctx.lang(),
        "mainEntity": {"@id": t.iri},
    });
    serde_json::to_string_pretty(&json!({
        "@context": "https://schema.org",
        "@graph": [page, term],
    }))
    .unwrap_or_default()
}

fn document_jsonld(ctx: &Ctx<'_>, d: &view::DocumentView) -> String {
    let mut set = json!({
        "@type": ["DefinedTermSet", "Dataset"],
        "@id": d.iri,
        "name": d.title,
        "url": d.url,
        "inLanguage": ctx.lang(),
    });
    if let Some(desc) = &d.description {
        set["description"] = json!(oneline(&desc.value));
    }
    if let Some(l) = &d.license {
        set["license"] = json!(l);
    }
    if let Some(v) = &d.version {
        set["version"] = json!(v);
    }
    if let Some(m) = &d.modified {
        set["dateModified"] = json!(m);
    }
    let distributions: Vec<serde_json::Value> = d
        .siblings
        .iter()
        .map(|s| {
            json!({
                "@type": "DataDownload",
                "encodingFormat": s.media_type,
                "contentUrl": s.url,
            })
        })
        .collect();
    set["distribution"] = json!(distributions);
    serde_json::to_string_pretty(&json!({"@context": "https://schema.org", "@graph": [set]}))
        .unwrap_or_default()
}

fn site_jsonld(ctx: &Ctx<'_>, s: &view::SiteView) -> String {
    let datasets: Vec<serde_json::Value> = s
        .namespaces
        .iter()
        .map(|n| json!({"@type": "Dataset", "@id": n.iri, "name": n.title, "url": n.url}))
        .collect();
    serde_json::to_string_pretty(&json!({
        "@context": "https://schema.org",
        "@type": "DataCatalog",
        "name": s.title,
        "url": s.base_url,
        "inLanguage": ctx.lang(),
        "dataset": datasets,
    }))
    .unwrap_or_default()
}

impl view::TermView {
    /// The RDF types worth restating in structured data, as CURIEs.
    fn types_for_jsonld(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Render every HTML page plus the theme assets.
/// Every HTML page of one namespace.
///
/// Split out of `render` so that a snapshot can call it with a plan whose
/// mount carries a version segment. The pages are then identical to the
/// latest ones except for that prefix, which is what makes a snapshot a
/// release rather than a second design.
#[allow(clippy::too_many_arguments)]
pub fn namespace_pages(
    ctx: &Ctx<'_>,
    ns: &crate::site::NamespacePlan,
    env: &Environment<'_>,
    site: &view::SiteView,
    site_value: &Value,
    licence: &Option<String>,
    out: &mut super::Output,
) -> Result<()> {
    let document = ns
        .document
        .as_deref()
        .and_then(|iri| ctx.release.document(iri));

    if let Some(doc) = document {
        let dv = view::document(ctx, ns, doc)?;
        let path = ctx.plan.document_path(ns, Rep::Html);
        let page = Page {
            kind: "document",
            title: page_title(&dv.title, None, &site.title),
            assets: format!("{}assets/", relative_root(&path)),
            root: relative_root(&path),
            llms_url: dv.llms_url.clone(),
            license: doc.header.license.clone().or_else(|| licence.clone()),
            license_label: doc
                .header
                .license
                .as_deref()
                .or(licence.as_deref())
                .map(licence_label),
            collection_url: None,
            cite_as: ctx.config.site.cite_as,
            jsonld: document_jsonld(ctx, &dv),
            banner: banner_for(Some(doc), ctx.lang()),
            hierarchy_html: hierarchy_html(&dv.hierarchy, &site.base_url, &relative_root(&path)),
        };
        let template = env
            .get_template("document.html.jinja")
            .context("loading document.html.jinja")?;
        let html = template
            .render(minijinja::context! {
                site => site_value.clone(),
                document => Value::from_serialize(&dv),
                page => Value::from_serialize(&page),
            })
            .context("rendering the document page")?;
        out.add(path, html);
    }

    for t in ctx.release.local_terms().filter(|t| t.namespace == ns.iri) {
        let tv = view::term(ctx, ns, t)?;
        let doc = ctx.document_of(t);
        let collection_url = doc
            .and_then(|d| ctx.namespace_of_document(d))
            .map(|dns| ctx.plan.document_url(dns, Rep::Html));
        let path = ctx.plan.term_path(ns, &t.local_name, Rep::Html);
        let page = Page {
            kind: "term",
            title: page_title(&tv.label, tv.curie.as_deref(), &site.title),
            assets: format!("{}assets/", relative_root(&path)),
            root: relative_root(&path),
            llms_url: ctx.plan.llms_url(ns),
            license: doc
                .and_then(|d| d.header.license.clone())
                .or_else(|| licence.clone()),
            license_label: doc
                .and_then(|d| d.header.license.clone())
                .or_else(|| licence.clone())
                .as_deref()
                .map(licence_label),
            collection_url,
            cite_as: ctx.config.site.cite_as,
            jsonld: term_jsonld(ctx, &tv, doc.map(|d| d.iri.as_str())),
            banner: banner_for(doc, ctx.lang()),
            hierarchy_html: String::new(),
        };
        let template = env
            .get_template("term.html.jinja")
            .context("loading term.html.jinja")?;
        let html = template
            .render(minijinja::context! {
                site => site_value.clone(),
                term => Value::from_serialize(&tv),
                page => Value::from_serialize(&page),
            })
            .with_context(|| format!("rendering {}", t.iri))?;
        out.add(path, html);
    }

    Ok(())
}

/// The pages of one namespace, with the surrounding site view built here.
///
/// A snapshot calls this with a plan whose mount carries a version segment.
/// The site view is rebuilt from that plan on purpose: inside an archived
/// release, the link to its own namespace should stay in the archive, while
/// namespaces this release does not version still point at their live URLs,
/// because there is no archived copy of them to point at.
pub fn namespace(
    ctx: &Ctx<'_>,
    ns: &crate::site::NamespacePlan,
    env: &Environment<'_>,
    out: &mut super::Output,
) -> Result<()> {
    let site = view::site(ctx);
    let site_value = Value::from_serialize(&site);
    let licence = ctx
        .release
        .root_document()
        .and_then(|d| d.header.license.clone());
    namespace_pages(ctx, ns, env, &site, &site_value, &licence, out)
}

/// The PDF template: the bundled one, or a `--theme` override of it.
///
/// Symmetric with the HTML templates. A publisher who wants a different PDF
/// edits one `.typ` file and nothing else.
pub fn pdf_template(theme_dir: Option<&camino::Utf8Path>) -> String {
    if let Some(dir) = theme_dir {
        let candidate = dir.join("pdf").join("spec.typ");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            return text;
        }
    }
    include_str!("../../assets/pdf/spec.typ").to_owned()
}

pub fn render(
    ctx: &Ctx<'_>,
    env: &Environment<'_>,
    tokens: &Tokens,
    theme_dir: Option<&camino::Utf8Path>,
    out: &mut super::Output,
) -> Result<()> {
    let site = view::site(ctx);
    let site_value = Value::from_serialize(&site);

    // Theme assets. tokens.css is generated from the token file so that the
    // custom properties and the checked values cannot drift apart.
    out.add(
        "assets/tokens.css",
        crate::theme::tokens_css(tokens, ctx.plan.color_scheme),
    );
    for (path, content) in default_assets() {
        if path.ends_with("tokens.css") {
            continue;
        }
        out.add(
            format!("assets/{}", path.trim_start_matches("assets/")),
            content,
        );
    }
    // A theme's own assets, over the embedded ones by path. Same-named files
    // replace rather than merge, which is what "overlaid by file name"
    // already means for templates.
    theme_assets(theme_dir, out)?;

    let licence = ctx
        .release
        .root_document()
        .and_then(|d| d.header.license.clone());

    for ns in &ctx.plan.namespaces {
        namespace_pages(ctx, ns, env, &site, &site_value, &licence, out)?;
    }

    // Site index and the 404 page, which Cloudflare and GitHub Pages both need.
    let page = Page {
        kind: "index",
        title: site.title.clone(),
        assets: "assets/".to_owned(),
        root: String::new(),
        llms_url: site.llms_url.clone(),
        license: licence.clone(),
        license_label: licence.as_deref().map(licence_label),
        collection_url: None,
        cite_as: false,
        jsonld: site_jsonld(ctx, &site),
        banner: banner_for(ctx.release.root_document(), ctx.lang()),
        hierarchy_html: String::new(),
    };
    // The 404 page is the one page with no address of its own: a host answers
    // it at whatever path was asked for, so a document-relative href on it
    // resolves against that path and lands nowhere. Its links are therefore
    // root-relative, and root-relative means knowing where the root is:
    // `/` for a site at a domain root, `/repo/` for a copy under a project
    // path. `site::base_path` is that, taken from `base_url` unless
    // `site.base_path` says otherwise (`docs/cli.md`, "Configuration file").
    let base_path = crate::site::base_path(ctx.config);
    let not_found = Page {
        kind: "404",
        assets: format!("{base_path}assets/"),
        root: base_path,
        ..page.clone()
    };
    for (template_name, path, page) in [
        ("index.html.jinja", "index.html", &page),
        ("404.html.jinja", "404.html", &not_found),
    ] {
        let template = env
            .get_template(template_name)
            .with_context(|| format!("loading {template_name}"))?;
        let html = template
            .render(minijinja::context! {
                site => site_value.clone(),
                page => Value::from_serialize(page),
            })
            .with_context(|| format!("rendering {template_name}"))?;
        out.add(path, html);
    }

    Ok(())
}
