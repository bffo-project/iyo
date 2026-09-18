//! Where every file goes and what its URL is.
//!
//! The mount of a namespace mirrors the path of its IRI by default, even when
//! the documents are served from another host. That is what makes the built
//! tree identical under both hostname layouts, so choosing between them is a
//! configuration change and not a rebuild (`docs/output-convention.md`,
//! "Configuration that changes layout, URLs or negotiation").

use crate::config::Config;
use crate::model::{Namespace, Release};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UrlStyle {
    /// `/Format` served by `Format.html`, siblings `Format.md`, `Format.ttl`.
    Flat,
    /// `/Format/` served by `Format/index.html`, siblings `Format/index.md`.
    Dir,
}

/// Which colour schemes a build publishes.
///
/// `Auto` is the default and what every build did before this existed: the
/// light palette, with a `prefers-color-scheme: dark` block so a reader whose
/// system asks for dark gets dark. Forcing one is for a site that has to
/// match surroundings which do not offer the other. BFFO is the case: its
/// website has no dark mode at all, so a reader on a dark system currently
/// gets a light site and dark documentation.
///
/// Only the schemes actually published are gated. Refusing to build over a
/// contrast pair in a palette the site never serves would be a failure
/// earned by nothing, which is the shape of defect this project keeps
/// finding; the unpublished palette is checked anyway and reported as a
/// note, so flipping to `auto` later is not a surprise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// Both, with the reader's system choosing.
    #[default]
    Auto,
    /// Light only.
    Light,
    /// Dark only.
    Dark,
}

impl ColorScheme {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// The scheme names this build publishes, as `theme::PairResult::scheme`
    /// spells them.
    pub fn published(self) -> &'static [&'static str] {
        match self {
            Self::Auto => &["light", "dark"],
            Self::Light => &["light"],
            Self::Dark => &["dark"],
        }
    }
}

/// The path a site is served from, with a leading and trailing slash.
///
/// Every page but one links relatively and so works at any depth without
/// knowing this. The exception is `404.html`, which has no address of its
/// own: a host answers it at whatever path was asked for, so a
/// document-relative href on it resolves against *that* path and lands
/// nowhere. Its links have to be root-relative, and root-relative means
/// knowing where the root is.
///
/// Defaults to the path component of `base_url`, which is right whenever the
/// documents are served where their IRIs say they are. `config.site.base_path`
/// overrides it for the case the default cannot reach: a copy of the
/// documentation at `https://user.github.io/repo/` whose terms are still
/// identified by `https://bffo.org/ontology/…`. Identity and location are
/// different facts and only one of them is in `base_url`.
pub fn base_path(config: &crate::config::Config) -> String {
    let raw = config.site.base_path.clone().unwrap_or_else(|| {
        // The path component of the base URL: everything from the first `/`
        // after the authority. A base URL with no path at all is the root.
        config
            .site
            .base_url
            .split_once("//")
            .and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_owned()))
            .unwrap_or_else(|| "/".to_owned())
    });
    let trimmed = raw.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        format!("/{trimmed}/")
    }
}

/// Where a *navigation* link points, as distinct from what a term's identity
/// is. The identity never changes: `rel="canonical"` and `rel="cite-as"` are
/// the term IRI under either value, and so is everything in the manifest.
///
/// This exists because a site's pages can outlive its negotiation. The
/// default writes a term link as the term IRI, which is the right thing
/// wherever the convention's negotiation is implemented: the link a reader
/// follows and the identifier they would cite are then the same string, which
/// is most of the point of the convention. On a host that serves files and
/// nothing else, that link is a 404: `/ontology/Format` has no file, only
/// `Format.html` does. Crawled on the reference ontology, 520 of 6,356
/// internal links, every one of them a term.
///
/// `File` points navigation at the file instead, and leaves identity alone.
/// A reader browsing a GitHub Pages copy, a `file://` checkout or a
/// preview build gets working links; the pages still say the term is
/// `https://bffo.org/ontology/Format`, and a machine reading the RDF or the
/// `Link` header still sees only that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkStyle {
    /// Navigation links are term IRIs. Correct where negotiation exists.
    #[default]
    Iri,
    /// Navigation links are the files. Correct where it does not.
    File,
}

impl LinkStyle {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "iri" => Some(Self::Iri),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// Which representation of a term or document is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rep {
    Html,
    Markdown,
    Turtle,
    JsonLd,
}

impl Rep {
    pub fn extension(self) -> &'static str {
        match self {
            Rep::Html => ".html",
            Rep::Markdown => ".md",
            Rep::Turtle => ".ttl",
            Rep::JsonLd => ".jsonld",
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Rep::Html => "text/html",
            Rep::Markdown => "text/markdown",
            Rep::Turtle => "text/turtle",
            Rep::JsonLd => "application/ld+json",
        }
    }

    pub fn all() -> [Rep; 4] {
        [Rep::Html, Rep::Markdown, Rep::Turtle, Rep::JsonLd]
    }

    /// The representations this build actually writes per term.
    ///
    /// A page must not advertise a sibling that does not exist: a dangling
    /// `rel="alternate"` is worse than a missing one, because a client that
    /// negotiates for it gets a 404 instead of falling back. Everything in
    /// this list is written for every term, in every layout.
    pub fn produced() -> [Rep; 4] {
        [Rep::Html, Rep::Markdown, Rep::Turtle, Rep::JsonLd]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespacePlan {
    pub iri: String,
    /// Directory under the site root, `""` or ending in `/`.
    pub mount: String,
    pub style: UrlStyle,
    /// Basename of the whole-namespace serialisations.
    pub stem: String,
    pub prefix: Option<String>,
    pub resolver_prefix: Option<String>,
    pub document: Option<String>,
    pub reserved: Vec<String>,
    /// Local names that must use the directory layout because another name in
    /// the same namespace differs from them only by case.
    pub cased: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub base_url: String,
    pub lang: String,
    /// Where navigation links point. Identity is unaffected either way; see
    /// [`LinkStyle`].
    ///
    /// Which colour schemes this build publishes.
    ///
    /// Not serialised, for the same reason as `link_style` below.
    #[serde(skip)]
    pub color_scheme: ColorScheme,
    /// Not serialised. `Plan` is part of the theme contract, and adding a
    /// field to it would change every `model --json` document and so every
    /// build digest, including for the builds that never asked for this.
    /// Nothing downstream of the contract needs it: it decides an href the
    /// renderer has already resolved by the time a theme sees it.
    #[serde(skip)]
    pub link_style: LinkStyle,
    pub namespaces: Vec<NamespacePlan>,
    /// Basename (no extension) of a namespace's own Markdown document.
    ///
    /// `index` everywhere, except under `--md-frontmatter hugo`, where it is
    /// `_index`. Hugo treats a directory containing `index.md` as a *leaf
    /// bundle*: every sibling `.md` becomes a resource of that one page
    /// instead of a page of its own, which is why a Hugo build of an
    /// unmodified iyo site turns 57 term pages into 1. `_index.md` instead
    /// makes the directory a *branch bundle* (a section), so the terms
    /// beside it render as pages. This is decided once, here, and every
    /// consumer of a namespace document's path — the file write in
    /// `render::namespace_files`, `manifest.json`'s
    /// `representations[].namespace_file`, the `llms.txt` and Markdown links
    /// that name it, and `render::audit`'s set of known files — reads it off
    /// `document_path` rather than recomputing the filename, so none of them
    /// can disagree about what was actually written.
    md_document_stem: &'static str,
}

/// The path part of an IRI, as a mount directory.
///
/// `https://bffo.org/ontology/` gives `ontology/`, `https://example.org/`
/// gives `""`, and a hash namespace `http://ex.org/vocab#` gives `vocab/`.
pub fn mount_from_iri(iri: &str) -> String {
    let without_scheme = iri.split_once("//").map(|(_, r)| r).unwrap_or(iri);
    let path = match without_scheme.find('/') {
        Some(i) => &without_scheme[i + 1..],
        None => "",
    };
    let path = path.trim_end_matches('#');
    if path.is_empty() {
        String::new()
    } else if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

fn stem_for(ns: &Namespace) -> String {
    if let Some(p) = &ns.prefix {
        return p.clone();
    }
    let mount = mount_from_iri(&ns.iri);
    mount
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("vocabulary")
        .to_owned()
}

/// Basename (no extension) a namespace's own Markdown document is written
/// under: see the doc comment on `Plan::md_document_stem`.
///
/// A plain string comparison, not `render::markdown::FrontMatter::parse`: an
/// unrecognised `md_frontmatter` value simply is not `"hugo"` here, and the
/// build already refuses to write anything once `render::namespace_files`
/// runs that same value through the real parser and gets an error back, so
/// an invalid value can only fail loudly, never fall back to this silently.
fn md_document_stem(config: &Config) -> &'static str {
    if config.site.md_frontmatter == "hugo" {
        "_index"
    } else {
        "index"
    }
}

impl Plan {
    /// File stems the site itself owns at a mount.
    ///
    /// A vocabulary is free to define a term called `index`, and BFFO defines
    /// two. Written flat, `index.html` would be both the concept's page and
    /// the scheme's own page, and one would silently overwrite the other.
    /// Such a term falls back to the directory layout: its file becomes
    /// `index/index.html` and its URL `…/index/`, so the term IRI still
    /// resolves (through one trailing-slash redirect on hosts that add it)
    /// and the scheme keeps the directory URL.
    pub fn is_reserved_stem(ns: &NamespacePlan, local: &str) -> bool {
        local == "index" || (ns.mount.is_empty() && local == "404")
    }

    /// Local names that would collide on a case-insensitive filesystem.
    ///
    /// `bffo:Format`, `bffo:FormatVersion` and `bffo:formatVersion` are
    /// ordinary OWL naming: classes in upper camel case, properties in lower.
    /// Written flat, `FormatVersion.html` and `formatVersion.html` are the
    /// same file on macOS and Windows, and the build silently wrote one over
    /// the other: 723 files on disk where it reported 727. The IRIs are
    /// distinct and both must resolve, so the answer is not to rename
    /// anything but to give all but the first of each colliding group the
    /// directory layout, where the names no longer meet.
    fn colliding(locals: &[String]) -> Vec<String> {
        let mut groups: std::collections::BTreeMap<String, Vec<&String>> =
            std::collections::BTreeMap::new();
        for l in locals {
            groups.entry(l.to_lowercase()).or_default().push(l);
        }
        let mut out = Vec::new();
        for (_, mut names) in groups {
            if names.len() < 2 {
                continue;
            }
            names.sort();
            // The first keeps the flat layout, so the common case is
            // unchanged and the choice does not depend on input order.
            out.extend(names.into_iter().skip(1).cloned());
        }
        out.sort();
        out
    }

    fn style_for(ns: &NamespacePlan, local: &str) -> UrlStyle {
        if Self::is_reserved_stem(ns, local) || ns.cased.iter().any(|c| c == local) {
            UrlStyle::Dir
        } else {
            ns.style
        }
    }

    pub fn new(release: &Release, config: &Config) -> Self {
        let namespaces = release
            .namespaces
            .iter()
            .map(|ns| {
                let overrides = config.namespaces.get(&ns.iri);
                let mount = overrides
                    .and_then(|o| o.mount.clone())
                    .map(|m| {
                        let m = m.trim_start_matches('/').to_owned();
                        if m.is_empty() || m.ends_with('/') {
                            m
                        } else {
                            format!("{m}/")
                        }
                    })
                    .unwrap_or_else(|| mount_from_iri(&ns.iri));
                let style = match overrides.and_then(|o| o.url_style.as_deref()) {
                    Some("dir") => UrlStyle::Dir,
                    _ => UrlStyle::Flat,
                };
                let locals: Vec<String> = release
                    .local_terms()
                    .filter(|t| t.namespace == ns.iri)
                    .map(|t| t.local_name.clone())
                    .collect();
                NamespacePlan {
                    iri: ns.iri.clone(),
                    mount,
                    style,
                    stem: overrides
                        .and_then(|o| o.stem.clone())
                        .unwrap_or_else(|| stem_for(ns)),
                    prefix: ns.prefix.clone(),
                    resolver_prefix: overrides.and_then(|o| o.resolver_prefix.clone()),
                    document: ns.document.clone(),
                    reserved: ns.reserved.clone(),
                    cased: Self::colliding(&locals),
                }
            })
            .collect();
        Self {
            base_url: config.site.base_url.clone(),
            lang: config.site.lang.clone(),
            link_style: LinkStyle::parse(&config.site.link_style).unwrap_or_default(),
            color_scheme: ColorScheme::parse(&config.site.color_scheme).unwrap_or_default(),
            namespaces,
            md_document_stem: md_document_stem(config),
        }
    }

    pub fn namespace(&self, iri: &str) -> Option<&NamespacePlan> {
        self.namespaces.iter().find(|n| n.iri == iri)
    }

    /// Path of a term's representation, relative to the output root.
    pub fn term_path(&self, ns: &NamespacePlan, local: &str, rep: Rep) -> String {
        match Self::style_for(ns, local) {
            UrlStyle::Flat => format!("{}{}{}", ns.mount, local, rep.extension()),
            UrlStyle::Dir => format!("{}{}/index{}", ns.mount, local, rep.extension()),
        }
    }

    /// URL of a term's representation. HTML is extensionless in `flat` style
    /// and a directory URL in `dir` style, which is what makes the term IRI
    /// and the document URL line up.
    pub fn term_url(&self, ns: &NamespacePlan, local: &str, rep: Rep) -> String {
        match (Self::style_for(ns, local), rep) {
            (UrlStyle::Flat, Rep::Html) => format!("{}{}{}", self.base_url, ns.mount, local),
            (UrlStyle::Flat, _) => {
                format!("{}{}{}{}", self.base_url, ns.mount, local, rep.extension())
            }
            (UrlStyle::Dir, Rep::Html) => format!("{}{}{}/", self.base_url, ns.mount, local),
            (UrlStyle::Dir, _) => format!(
                "{}{}{}/index{}",
                self.base_url,
                ns.mount,
                local,
                rep.extension()
            ),
        }
    }

    /// Where a link to this term should send a browser.
    ///
    /// The same string as `term_url(.., Rep::Html)` under the default
    /// `LinkStyle::Iri`, so a build that does not ask for anything else is
    /// byte-identical. Under `LinkStyle::File` it is the document that
    /// actually exists, which is what makes the pages browsable on a host
    /// that cannot negotiate.
    ///
    /// Only navigation uses this. `term_url` remains what identity is built
    /// from, and the two are deliberately separate names rather than a flag
    /// on one function: every call site has to decide which it means, and
    /// the compiler asks.
    pub fn term_doc_url(&self, ns: &NamespacePlan, local: &str) -> String {
        match self.link_style {
            LinkStyle::Iri => self.term_url(ns, local, Rep::Html),
            LinkStyle::File => {
                format!("{}{}", self.base_url, self.term_path(ns, local, Rep::Html))
            }
        }
    }

    /// A term's canonical request path, relative to the site root: no host,
    /// a leading `/`, and the trailing `/` `dir` style uses (including the
    /// per-namespace collision cases `NamespacePlan::cased` records).
    ///
    /// This exists for `--md-frontmatter hugo`'s `url:` front matter field
    /// (`markdown::term_front_matter`) alone. Hugo derives a page's URL from
    /// its source filename and lowercases that derived slug, which folds
    /// `Format` and a same-named-but-cased sibling onto one path — the same
    /// collision `NamespacePlan::cased`, the dir-term and case-fold rules
    /// exist to keep apart, reappearing in a consumer that does its own
    /// path-casing. Setting `url` to this exact, case-preserving path is what
    /// keeps them apart in Hugo's output too. MkDocs and Jekyll use the source
    /// filename verbatim as the URL, so neither needs this and neither gets
    /// it.
    pub fn term_request_path(&self, ns: &NamespacePlan, local: &str) -> String {
        match Self::style_for(ns, local) {
            UrlStyle::Flat => format!("/{}{}", ns.mount, local),
            UrlStyle::Dir => format!("/{}{}/", ns.mount, local),
        }
    }

    /// Path of a namespace-level file.
    ///
    /// Markdown alone consults `md_document_stem`: HTML's own filename is
    /// never in question (no generator this flag targets renames it), and
    /// Hugo is the only one of the three with a bundle convention for the
    /// Markdown source to collide with.
    ///
    /// One consequence, not a bug: a resolver that negotiates the bare
    /// mount by media type (the `dcmi-ns` resolver config in
    /// `adapter::dcmi` is the one this build emits) sends an
    /// `Accept: text/markdown` request for `/ontology/` to
    /// `/ontology/_index.md` once this returns that name, where before the
    /// flag it would have gone to `/ontology/index.md`. That only changes
    /// for a publisher who opted into `--md-frontmatter hugo`, i.e. one
    /// already feeding the tree to Hugo rather than serving it as iyo wrote
    /// it, so the new target is consistent rather than broken.
    pub fn document_path(&self, ns: &NamespacePlan, rep: Rep) -> String {
        match rep {
            Rep::Turtle | Rep::JsonLd => format!("{}{}{}", ns.mount, ns.stem, rep.extension()),
            Rep::Markdown => format!("{}{}{}", ns.mount, self.md_document_stem, rep.extension()),
            Rep::Html => format!("{}index{}", ns.mount, rep.extension()),
        }
    }

    pub fn document_url(&self, ns: &NamespacePlan, rep: Rep) -> String {
        match rep {
            Rep::Html => format!("{}{}", self.base_url, ns.mount),
            _ => format!("{}{}", self.base_url, self.document_path(ns, rep)),
        }
    }

    pub fn llms_path(&self, ns: &NamespacePlan) -> String {
        format!("{}llms.txt", ns.mount)
    }

    pub fn llms_url(&self, ns: &NamespacePlan) -> String {
        format!("{}{}", self.base_url, self.llms_path(ns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_term_named_index_does_not_overwrite_the_document_page() {
        let ns = NamespacePlan {
            iri: "https://example.org/v/".to_owned(),
            mount: "v/".to_owned(),
            style: UrlStyle::Flat,
            stem: "v".to_owned(),
            prefix: None,
            resolver_prefix: None,
            document: None,
            reserved: Vec::new(),
            cased: Vec::new(),
        };
        let plan = Plan {
            link_style: LinkStyle::default(),
            color_scheme: ColorScheme::default(),
            base_url: "https://example.org/".to_owned(),
            lang: "en".to_owned(),
            namespaces: vec![ns.clone()],
            md_document_stem: "index",
        };
        assert_eq!(plan.document_path(&ns, Rep::Html), "v/index.html");
        assert_eq!(
            plan.term_path(&ns, "index", Rep::Html),
            "v/index/index.html"
        );
        assert_eq!(
            plan.term_url(&ns, "index", Rep::Html),
            "https://example.org/v/index/"
        );
        // Any other term is unaffected.
        assert_eq!(plan.term_path(&ns, "square", Rep::Html), "v/square.html");
    }

    #[test]
    fn mounts_mirror_the_iri_path() {
        assert_eq!(mount_from_iri("https://bffo.org/ontology/"), "ontology/");
        assert_eq!(
            mount_from_iri("https://bffo.org/vocabulary/category/"),
            "vocabulary/category/"
        );
        assert_eq!(mount_from_iri("https://example.org/"), "");
        assert_eq!(mount_from_iri("http://ex.org/vocab#"), "vocab/");
        assert_eq!(mount_from_iri("http://purl.org/dc/terms/"), "dc/terms/");
    }

    #[test]
    fn only_hugo_gets_the_underscore_index_stem() {
        let mut config = Config::default();
        for style in ["none", "mkdocs", "jekyll", "bogus"] {
            config.site.md_frontmatter = style.to_owned();
            assert_eq!(md_document_stem(&config), "index", "style {style:?}");
        }
        config.site.md_frontmatter = "hugo".to_owned();
        assert_eq!(md_document_stem(&config), "_index");
    }

    #[test]
    fn hugo_names_the_namespace_document_underscore_index() {
        let ns = NamespacePlan {
            iri: "https://example.org/v/".to_owned(),
            mount: "v/".to_owned(),
            style: UrlStyle::Flat,
            stem: "v".to_owned(),
            prefix: None,
            resolver_prefix: None,
            document: None,
            reserved: Vec::new(),
            cased: Vec::new(),
        };
        let plan = Plan {
            link_style: LinkStyle::default(),
            color_scheme: ColorScheme::default(),
            base_url: "https://example.org/".to_owned(),
            lang: "en".to_owned(),
            namespaces: vec![ns.clone()],
            md_document_stem: "_index",
        };
        assert_eq!(plan.document_path(&ns, Rep::Markdown), "v/_index.md");
        // Every other representation is unaffected.
        assert_eq!(plan.document_path(&ns, Rep::Html), "v/index.html");
        assert_eq!(plan.document_path(&ns, Rep::Turtle), "v/v.ttl");
        assert_eq!(
            plan.document_url(&ns, Rep::Markdown),
            "https://example.org/v/_index.md"
        );
    }

    #[test]
    fn term_request_path_is_absolute_case_exact_and_style_aware() {
        let ns = NamespacePlan {
            iri: "https://example.org/v/".to_owned(),
            mount: "v/".to_owned(),
            style: UrlStyle::Flat,
            stem: "v".to_owned(),
            prefix: None,
            resolver_prefix: None,
            document: None,
            reserved: Vec::new(),
            cased: vec!["formatVersion".to_owned()],
        };
        let plan = Plan {
            link_style: LinkStyle::default(),
            color_scheme: ColorScheme::default(),
            base_url: "https://example.org/".to_owned(),
            lang: "en".to_owned(),
            namespaces: vec![ns.clone()],
            md_document_stem: "index",
        };
        // Flat style: no trailing slash, case preserved.
        assert_eq!(plan.term_request_path(&ns, "Format"), "/v/Format");
        // Forced to `dir` by a same-namespace case collision: trailing
        // slash, case still preserved.
        assert_eq!(
            plan.term_request_path(&ns, "formatVersion"),
            "/v/formatVersion/"
        );
    }
}
