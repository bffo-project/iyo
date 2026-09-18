//! Configuration, and the defaults that make configuration optional.
//!
//! Precedence is flags, then the `IYO_*` environment variables clap reads
//! into those same flags, then `./iyo.toml`, then the built-in defaults
//! (`docs/cli.md`, "Configuration file: resolution order and the accepted
//! tables").
//!
//! Every table refuses keys it does not know. A misspelled key used to be
//! accepted in silence, so the setting simply did not apply and nothing
//! said why; one auditor lost time to exactly that.

use anyhow::{Context, Result};
use camino::Utf8Path;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Site {
    /// Absolute URL of the site root. Always stored with a trailing slash.
    pub base_url: String,
    pub lang: String,
    /// Licence of the documentation, distinct from the vocabulary licence
    /// which comes from `dcterms:license` in the RDF.
    pub doc_license: Option<String>,
    pub title: Option<String>,
    /// Whether the identity IRI resolves back to the page that claims it.
    /// FAIR Signposting makes `rel="cite-as"` a conformance failure when it
    /// does not, so a build for a host that cannot negotiate leaves it off.
    pub cite_as: bool,
    /// When to write a versioned snapshot: `version-iri`, `all` or `none`
    /// (`docs/output-convention.md`, "Versioned snapshots").
    pub snapshots: String,
    /// The release string to use when the RDF carries none.
    pub release: Option<String>,
    /// Whether the newest snapshot is a copy of what `B/` serves, or `B/` is
    /// a living document that moves on after a release (DCMI practice).
    /// Recorded in `versions.json` either way; it changes no bytes here.
    pub latest: String,
    /// Hosts to compile the manifest for. Empty by default: the manifest is
    /// host-neutral and always written, and a host configuration is a
    /// projection a publisher asks for.
    pub hosts: Vec<String>,
    /// Write the Typst template and data for a PDF of each namespace.
    pub pdf: bool,
    /// The previous release, so the build can write a changelog. Paths,
    /// directories or globs, read the same way as the inputs, because a
    /// release is usually several files and was never one.
    pub previous: Vec<String>,
    /// YAML front matter to prefix per-term Markdown with, so a publisher
    /// can drop the tree into that generator's content directory:
    /// `none`, `hugo`, `mkdocs` or `jekyll` (`docs/output-convention.md`,
    /// "File layout per term"). `none` is the default and changes no bytes.
    pub md_frontmatter: String,
    /// Where a navigation link points: `iri` (the default, and the term IRI)
    /// or `file` (the document that exists). Identity is unaffected either
    /// way; see `site::LinkStyle`.
    pub link_style: String,
    /// The path the site is served from, when that is not the path in
    /// `base_url`. Only the 404 page needs it; see `site::base_path`.
    pub base_path: Option<String>,
    /// Path prefixes on this origin that belong to something other than this
    /// build: another application sharing the hostname.
    ///
    /// A vocabulary often lives beside a website rather than instead of one.
    /// BFFO's documentation is served at `/ontology/` and `/vocabulary/` on a
    /// site whose `/formats/` and `/about/` are a separate program, so a
    /// header link to `/formats/` is external in every sense but the
    /// hostname -- and the auditor, which resolves any link under `base_url`
    /// against the files this build wrote, reported 2,097 broken internal
    /// links for six such links repeated across the site.
    ///
    /// No rule over URLs alone can tell that from a mistyped term, which is
    /// why this is declared rather than inferred.
    pub external_paths: Vec<String>,
    /// Offer the reader a light/dark control, which costs the site its
    /// "no JavaScript anywhere" property.
    ///
    /// Off by default, and the default is the point. Every theme's
    /// `tokens.css` already follows `prefers-color-scheme`, so a reader
    /// without this gets the scheme their operating system asks for. The
    /// control adds an override, and an override needs somewhere to remember
    /// the choice, and that is `localStorage` and therefore a script. A
    /// publisher who wants none should not have to opt out of one.
    pub theme_switch: bool,
    /// Which colour schemes to publish: `auto` (the default, both, with the
    /// reader's system choosing), `light` or `dark`. See `site::ColorScheme`.
    pub color_scheme: String,
}

impl Default for Site {
    fn default() -> Self {
        Self {
            base_url: "/".to_owned(),
            lang: "en".to_owned(),
            doc_license: None,
            title: None,
            cite_as: false,
            snapshots: "version-iri".to_owned(),
            release: None,
            latest: "release".to_owned(),
            hosts: Vec::new(),
            pdf: false,
            previous: Vec::new(),
            md_frontmatter: "none".to_owned(),
            link_style: "iri".to_owned(),
            base_path: None,
            external_paths: Vec::new(),
            theme_switch: false,
            color_scheme: "auto".to_owned(),
        }
    }
}

/// Per-namespace overrides. The default mount mirrors the namespace IRI path,
/// so that the same output tree serves either hostname layout (A3).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NamespaceConfig {
    /// Directory under the site root, for example `ontology/`.
    pub mount: Option<String>,
    /// Path under a redirect host that stands for this namespace.
    pub resolver_prefix: Option<String>,
    /// `flat` gives `/Format` with `Format.md` beside it; `dir` gives
    /// `/Format/` with `Format/index.md`. DCMI needs `dir`.
    pub url_style: Option<String>,
    /// Basename of the whole-namespace serialisations.
    pub stem: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Llms {
    /// Above this many terms, the index links per-kind files instead of
    /// listing every term (`docs/output-convention.md`, "Site-level files").
    pub max_terms: usize,
    /// A companion dataset's `llms.txt`, linked from the Related section.
    pub data_site: Option<String>,
}

impl Default for Llms {
    fn default() -> Self {
        Self {
            max_terms: 500,
            data_site: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub site: Site,
    pub inputs: Vec<String>,
    pub examples: Option<String>,
    pub narrative: Option<String>,
    pub namespaces: BTreeMap<String, NamespaceConfig>,
    pub llms: Llms,
}

impl Config {
    pub fn load(path: &Utf8Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let mut config: Config = toml::from_str(&text).map_err(|e| {
            crate::Failure::err(
                crate::exit::INPUT,
                format!("parsing {path}: {e}"),
                "the error above lists the keys this version accepts; iyo refuses keys it \
                 does not know rather than ignoring them, so a misspelling is reported \
                 instead of silently doing nothing",
            )
        })?;
        config.normalise();
        Ok(config)
    }

    /// The default configuration when no file is present.
    pub fn implicit() -> Self {
        Self::default()
    }

    pub fn normalise(&mut self) {
        if !self.site.base_url.ends_with('/') {
            self.site.base_url.push('/');
        }
    }
}
