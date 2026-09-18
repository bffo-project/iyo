//! Design tokens and the build-time contrast gate.
//!
//! The gate is the reason the theme layer exists: the deployment this tool
//! replaces shipped `#2680C2` on white at 4.24:1 and `#829AB1` at 2.91:1,
//! which no reviewer caught because nothing computed them.
//! Here the numbers are computed before any file is written.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const TOKENS_SCHEMA: &str = "iyo.tokens/1";
pub const REPORT_SCHEMA: &str = "0.1";

// ---------------------------------------------------------------------------
// Colour arithmetic. WCAG 2.2, definitions "relative luminance" and "contrast
// ratio" (https://www.w3.org/TR/WCAG22/, fetched 2026-09-10).
// ---------------------------------------------------------------------------

/// An 8-bit sRGB colour parsed from a `#rrggbb` token value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Srgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Why a token value could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorError {
    /// Not `#` followed by exactly six hexadecimal digits.
    Syntax(String),
}

impl std::fmt::Display for ColorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorError::Syntax(v) => {
                write!(f, "expected a #rrggbb colour, found `{v}`")
            }
        }
    }
}

impl Srgb {
    /// Parses `#rrggbb`. Three-digit and named forms are rejected on purpose:
    /// a token file is machine-written and the gate must not guess.
    pub fn parse(value: &str) -> Result<Self, ColorError> {
        let hex = value.strip_prefix('#').unwrap_or("");
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ColorError::Syntax(value.to_owned()));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digits");
        Ok(Srgb {
            r: byte(0),
            g: byte(2),
            b: byte(4),
        })
    }

    /// WCAG relative luminance, 0.0 for black and 1.0 for white.
    pub fn relative_luminance(self) -> f64 {
        fn channel(eight_bit: u8) -> f64 {
            let c = f64::from(eight_bit) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }
}

/// `(L1 + 0.05) / (L2 + 0.05)`, L1 the lighter. Range 1.0 to 21.0.
pub fn contrast_ratio(a: Srgb, b: Srgb) -> f64 {
    let (mut hi, mut lo) = (a.relative_luminance(), b.relative_luminance());
    if hi < lo {
        std::mem::swap(&mut hi, &mut lo);
    }
    (hi + 0.05) / (lo + 0.05)
}

// ---------------------------------------------------------------------------
// Roles and the pair table.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Text,
    TextMuted,
    Link,
    LinkVisited,
    Focus,
    Surface,
    SurfaceAlt,
    SurfaceCode,
    Border,
    BadgeFg,
    BadgeBg,
    BannerFg,
    BannerBg,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Text => "text",
            Role::TextMuted => "text-muted",
            Role::Link => "link",
            Role::LinkVisited => "link-visited",
            Role::Focus => "focus",
            Role::Surface => "surface",
            Role::SurfaceAlt => "surface-alt",
            Role::SurfaceCode => "surface-code",
            Role::Border => "border",
            Role::BadgeFg => "badge-fg",
            Role::BadgeBg => "badge-bg",
            Role::BannerFg => "banner-fg",
            Role::BannerBg => "banner-bg",
        }
    }

    pub const ALL: [Role; 13] = [
        Role::Text,
        Role::TextMuted,
        Role::Link,
        Role::LinkVisited,
        Role::Focus,
        Role::Surface,
        Role::SurfaceAlt,
        Role::SurfaceCode,
        Role::Border,
        Role::BadgeFg,
        Role::BadgeBg,
        Role::BannerFg,
        Role::BannerBg,
    ];
}

/// Which WCAG rule sets the threshold for a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Basis {
    /// 1.4.3 Contrast (Minimum), text below 24px, or below 18.5px bold.
    BodyText,
    /// 1.4.3 Large Text exception. No default pair uses it.
    LargeText,
    /// 1.4.11 Non-text Contrast: focus rings, borders, badge outlines.
    NonText,
    /// 1.4.1 Use of Color, technique G183. Only when links are not underlined.
    LinkVsText,
}

impl Basis {
    pub fn threshold(self) -> f64 {
        match self {
            Basis::BodyText => 4.5,
            Basis::LargeText | Basis::NonText | Basis::LinkVsText => 3.0,
        }
    }

    pub fn rule_id(self) -> &'static str {
        match self {
            Basis::BodyText => "theme.contrast-text",
            Basis::LargeText => "theme.contrast-large-text",
            Basis::NonText => "theme.contrast-non-text",
            Basis::LinkVsText => "theme.contrast-link-vs-text",
        }
    }
}

/// One foreground/background combination the templates actually produce.
#[derive(Debug, Clone, Copy)]
pub struct Pair {
    pub fg: Role,
    pub bg: Role,
    pub basis: Basis,
    /// Where it occurs, quoted in the finding so the message is actionable.
    pub site: &'static str,
}

/// The pairs the default templates produce. The gate never guesses the cross
/// product, because most of it never appears on a page.
///
/// This list is Rust, not configuration. A theme that introduces a surface of
/// its own cannot register the pairs that surface creates, and those pairs go
/// unchecked; adding them means adding rows here. `tokens.toml` has no escape
/// hatch for it, and declaring one is rejected, because the token file is
/// `deny_unknown_fields`.
pub const REQUIRED: &[Pair] = &[
    Pair {
        fg: Role::Text,
        bg: Role::Surface,
        basis: Basis::BodyText,
        site: "body copy, headings, dt and dd, table cells",
    },
    Pair {
        fg: Role::Text,
        bg: Role::SurfaceAlt,
        basis: Basis::BodyText,
        site: "nav, table head, card",
    },
    Pair {
        fg: Role::Text,
        bg: Role::SurfaceCode,
        basis: Basis::BodyText,
        site: "code and pre, IRIs and CURIEs",
    },
    Pair {
        fg: Role::Text,
        bg: Role::BannerBg,
        basis: Basis::BodyText,
        site: "status banner body",
    },
    Pair {
        fg: Role::TextMuted,
        bg: Role::Surface,
        basis: Basis::BodyText,
        site: "provenance line, footer, counts",
    },
    Pair {
        fg: Role::TextMuted,
        bg: Role::SurfaceAlt,
        basis: Basis::BodyText,
        site: "nav section labels",
    },
    Pair {
        fg: Role::TextMuted,
        bg: Role::SurfaceCode,
        basis: Basis::BodyText,
        site: "comments in example snippets",
    },
    Pair {
        fg: Role::TextMuted,
        bg: Role::BannerBg,
        basis: Basis::BodyText,
        site: "banner date line",
    },
    Pair {
        fg: Role::Link,
        bg: Role::Surface,
        basis: Basis::BodyText,
        site: "prose and index links",
    },
    Pair {
        fg: Role::Link,
        bg: Role::SurfaceAlt,
        basis: Basis::BodyText,
        site: "contents nav links",
    },
    Pair {
        fg: Role::Link,
        bg: Role::SurfaceCode,
        basis: Basis::BodyText,
        site: "linked term IRIs inside code",
    },
    Pair {
        fg: Role::Link,
        bg: Role::BannerBg,
        basis: Basis::BodyText,
        site: "latest-version link in the banner",
    },
    Pair {
        fg: Role::LinkVisited,
        bg: Role::Surface,
        basis: Basis::BodyText,
        site: "visited prose and index links",
    },
    Pair {
        fg: Role::LinkVisited,
        bg: Role::SurfaceAlt,
        basis: Basis::BodyText,
        site: "visited contents nav links",
    },
    Pair {
        fg: Role::LinkVisited,
        bg: Role::SurfaceCode,
        basis: Basis::BodyText,
        site: "visited linked IRIs inside code",
    },
    Pair {
        fg: Role::LinkVisited,
        bg: Role::BannerBg,
        basis: Basis::BodyText,
        site: "visited banner link",
    },
    Pair {
        fg: Role::BadgeFg,
        bg: Role::BadgeBg,
        basis: Basis::BodyText,
        site: "term-kind badge text",
    },
    Pair {
        fg: Role::BannerFg,
        bg: Role::BannerBg,
        basis: Basis::BodyText,
        site: "banner heading",
    },
    Pair {
        fg: Role::Focus,
        bg: Role::Surface,
        basis: Basis::NonText,
        site: "focus ring on the page",
    },
    Pair {
        fg: Role::Focus,
        bg: Role::SurfaceAlt,
        basis: Basis::NonText,
        site: "focus ring in the nav",
    },
    Pair {
        fg: Role::Focus,
        bg: Role::SurfaceCode,
        basis: Basis::NonText,
        site: "focus ring on a link in code",
    },
    Pair {
        fg: Role::Focus,
        bg: Role::BadgeBg,
        basis: Basis::NonText,
        site: "focus ring on a badge link",
    },
    Pair {
        fg: Role::Focus,
        bg: Role::BannerBg,
        basis: Basis::NonText,
        site: "focus ring on the banner link",
    },
    Pair {
        fg: Role::Border,
        bg: Role::Surface,
        basis: Basis::NonText,
        site: "table rules, hr, card and code edges",
    },
    Pair {
        fg: Role::Border,
        bg: Role::SurfaceAlt,
        basis: Basis::NonText,
        site: "table rules on the header row",
    },
    Pair {
        fg: Role::Border,
        bg: Role::SurfaceCode,
        basis: Basis::NonText,
        site: "code-block edge from inside",
    },
    Pair {
        fg: Role::Border,
        bg: Role::BadgeBg,
        basis: Basis::NonText,
        site: "badge outline",
    },
    Pair {
        fg: Role::Border,
        bg: Role::BannerBg,
        basis: Basis::NonText,
        site: "banner outline",
    },
];

/// Checked only when `[links] underline = false`.
pub const CONDITIONAL: &[Pair] = &[
    Pair {
        fg: Role::Link,
        bg: Role::Text,
        basis: Basis::LinkVsText,
        site: "link against surrounding prose",
    },
    Pair {
        fg: Role::LinkVisited,
        bg: Role::Text,
        basis: Basis::LinkVsText,
        site: "visited link against surrounding prose",
    },
];

// ---------------------------------------------------------------------------
// The token file.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokens {
    pub schema: String,
    pub meta: Meta,
    pub color: Schemes,
    pub links: Links,
    #[serde(rename = "font-family")]
    pub font_family: BTreeMap<String, String>,
    #[serde(rename = "font-size")]
    pub font_size: BTreeMap<String, String>,
    pub leading: BTreeMap<String, String>,
    pub space: BTreeMap<String, String>,
    pub layout: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schemes {
    pub light: Palette,
    pub dark: Palette,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Links {
    pub underline: bool,
    pub underline_thickness: String,
    pub underline_offset: String,
}

/// Every role is required in every scheme: a missing one is a parse error, so
/// there is no silent fallback to an unchecked colour.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Palette {
    pub text: String,
    pub text_muted: String,
    pub link: String,
    pub link_visited: String,
    pub focus: String,
    pub surface: String,
    pub surface_alt: String,
    pub surface_code: String,
    pub border: String,
    pub badge_fg: String,
    pub badge_bg: String,
    pub banner_fg: String,
    pub banner_bg: String,
}

impl Palette {
    pub fn raw(&self, role: Role) -> &str {
        match role {
            Role::Text => &self.text,
            Role::TextMuted => &self.text_muted,
            Role::Link => &self.link,
            Role::LinkVisited => &self.link_visited,
            Role::Focus => &self.focus,
            Role::Surface => &self.surface,
            Role::SurfaceAlt => &self.surface_alt,
            Role::SurfaceCode => &self.surface_code,
            Role::Border => &self.border,
            Role::BadgeFg => &self.badge_fg,
            Role::BadgeBg => &self.badge_bg,
            Role::BannerFg => &self.banner_fg,
            Role::BannerBg => &self.banner_bg,
        }
    }
}

// ---------------------------------------------------------------------------
// The gate.
// ---------------------------------------------------------------------------

/// Deliberately separate from `check::Severity` and `check::Finding`, which
/// they mirror. The gate runs before a release is loaded and reports on a
/// token file rather than on RDF, so it does not depend on the lint's types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairResult {
    pub scheme: &'static str,
    pub foreground: &'static str,
    pub background: &'static str,
    pub fg_value: String,
    pub bg_value: String,
    /// Full precision; the comparison uses this value, not the rounded one.
    pub ratio: f64,
    /// What a reader sees, two decimals, half-up.
    pub ratio_2dp: String,
    pub required: f64,
    pub basis: Basis,
    pub conditional: bool,
    pub passes: bool,
    pub site: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: &'static str,
    pub formula: &'static str,
    pub theme: String,
    pub underlined_links: bool,
    pub pairs: Vec<PairResult>,
    pub findings: Vec<Finding>,
    pub failed: usize,
}

fn round2(x: f64) -> String {
    format!("{:.2}", x)
}

/// Runs the gate over one token set.
///
/// Reads: `tokens.color.light`, `tokens.color.dark`, `tokens.links.underline`.
/// Checks: every pair in `REQUIRED` in both schemes, plus `CONDITIONAL` when
/// links are not underlined. Reports: one `PairResult` per pair with the
/// unrounded ratio and one `Finding` per failure. Fails the build when any
/// required pair is below its threshold and `--allow-contrast-failures` was
/// not passed.
pub fn gate(tokens: &Tokens) -> Report {
    let mut pairs = Vec::with_capacity(REQUIRED.len() * 2 + CONDITIONAL.len() * 2);
    let mut findings = Vec::new();
    let mut failed = 0usize;

    if tokens.schema != TOKENS_SCHEMA {
        findings.push(Finding {
            rule: "theme.token-schema".to_owned(),
            severity: Severity::Error,
            message: format!(
                "tokens declare schema `{}`, this build understands `{TOKENS_SCHEMA}`",
                tokens.schema
            ),
            subject: None,
            file: Some("tokens.toml".to_owned()),
        });
        failed += 1;
    }

    for (scheme, palette) in [("light", &tokens.color.light), ("dark", &tokens.color.dark)] {
        // Parse once per role so a syntax error is reported once, not per pair.
        let mut parsed: BTreeMap<Role, Srgb> = BTreeMap::new();
        for role in Role::ALL {
            match Srgb::parse(palette.raw(role)) {
                Ok(c) => {
                    parsed.insert(role, c);
                }
                Err(e) => {
                    findings.push(Finding {
                        rule: "theme.token-syntax".to_owned(),
                        severity: Severity::Error,
                        message: format!("color.{scheme}.{}: {e}", role.as_str()),
                        subject: Some(role.as_str().to_owned()),
                        file: Some("tokens.toml".to_owned()),
                    });
                    failed += 1;
                }
            }
        }
        if parsed.len() != Role::ALL.len() {
            continue;
        }

        let checked = REQUIRED.iter().map(|p| (p, false)).chain(
            CONDITIONAL
                .iter()
                .filter(|_| !tokens.links.underline)
                .map(|p| (p, true)),
        );

        for (pair, conditional) in checked {
            let fg = parsed[&pair.fg];
            let bg = parsed[&pair.bg];
            let ratio = contrast_ratio(fg, bg);
            let required = pair.basis.threshold();
            let passes = ratio >= required;
            if !passes {
                failed += 1;
                findings.push(Finding {
                    rule: pair.basis.rule_id().to_owned(),
                    severity: Severity::Error,
                    message: format!(
                        "{scheme}: {} {} on {} {} is {}:1, below {required}:1 ({})",
                        pair.fg.as_str(),
                        palette.raw(pair.fg),
                        pair.bg.as_str(),
                        palette.raw(pair.bg),
                        round2(ratio),
                        pair.site
                    ),
                    subject: Some(format!(
                        "color.{scheme}.{} on color.{scheme}.{}",
                        pair.fg.as_str(),
                        pair.bg.as_str()
                    )),
                    file: Some("tokens.toml".to_owned()),
                });
            }
            pairs.push(PairResult {
                scheme,
                foreground: pair.fg.as_str(),
                background: pair.bg.as_str(),
                fg_value: palette.raw(pair.fg).to_owned(),
                bg_value: palette.raw(pair.bg).to_owned(),
                ratio,
                ratio_2dp: round2(ratio),
                required,
                basis: pair.basis,
                conditional,
                passes,
                site: pair.site,
            });
        }
    }

    Report {
        schema_version: REPORT_SCHEMA,
        formula: "WCAG 2.2 relative luminance, (L1 + 0.05) / (L2 + 0.05)",
        theme: tokens.meta.name.clone(),
        underlined_links: tokens.links.underline,
        pairs,
        findings,
        failed,
    }
}

// ---------------------------------------------------------------------------
// Compilation to CSS.
// ---------------------------------------------------------------------------

fn scheme_block(p: &Palette, indent: &str) -> String {
    let mut s = String::new();
    for role in Role::ALL {
        let _ = writeln!(s, "{indent}--color-{}: {};", role.as_str(), p.raw(role));
    }
    s
}

/// Emits `assets/tokens.css`. Light on bare `:root`, dark under
/// Custom properties a stylesheet reads but nothing defines.
///
/// Without this check a renamed token fails in total silence: an unresolvable
/// `var()` makes the declaration invalid at computed-value time, so the
/// element simply keeps the browser default and the page still looks
/// plausible. Thirteen colour roles were proved against WCAG, written to
/// `tokens.css` as `--color-surface`, and read by the stylesheet as
/// `--surface`, so every page shipped in browser default colours while the
/// contrast gate reported success. A guarantee that is not wired to the
/// output is not a guarantee.
///
/// A `var(--x, fallback)` reference is not reported: a fallback is a
/// deliberate default, not an accident.
pub fn undefined_tokens(stylesheets: &[&str], tokens_css: &str) -> Vec<String> {
    let mut defined = std::collections::BTreeSet::new();
    for source in std::iter::once(&tokens_css).chain(stylesheets.iter()) {
        for (at, _) in source.match_indices("--") {
            let rest = &source[at..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                .unwrap_or(rest.len());
            let (name, tail) = (&rest[..end], rest[end..].trim_start());
            // A definition is `--name:`, a reference is `var(--name`.
            if tail.starts_with(':') && !source[..at].trim_end().ends_with("var(") {
                defined.insert(name.to_owned());
            }
        }
    }

    let mut missing = std::collections::BTreeSet::new();
    for source in stylesheets {
        for (at, _) in source.match_indices("var(--") {
            let rest = &source[at + 4..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                .unwrap_or(rest.len());
            let name = &rest[..end];
            let has_fallback = rest[end..].trim_start().starts_with(',');
            if !has_fallback && !defined.contains(name) {
                missing.insert(name.to_owned());
            }
        }
    }
    missing.into_iter().collect()
}

/// `prefers-color-scheme` guarded against a forced light theme, then the
/// `[data-theme]` overrides last so equal specificity resolves in their favour.
pub fn tokens_css(t: &Tokens, scheme: crate::site::ColorScheme) -> String {
    use crate::site::ColorScheme;
    let dark_only = scheme == ColorScheme::Dark;
    let mut s = String::new();
    let _ = writeln!(s, "/* Generated from tokens.toml by iyo. Do not edit. */");
    let _ = writeln!(s, "/* theme: {} {} */", t.meta.name, t.meta.version);
    if scheme != ColorScheme::Auto {
        let _ = writeln!(
            s,
            "/* published scheme: {} only */",
            if dark_only { "dark" } else { "light" }
        );
    }
    let _ = writeln!(s, ":root {{");
    let _ = writeln!(
        s,
        "  color-scheme: {};",
        if dark_only { "dark" } else { "light" }
    );
    s.push_str(&scheme_block(
        if dark_only {
            &t.color.dark
        } else {
            &t.color.light
        },
        "  ",
    ));
    for (k, v) in &t.font_family {
        let _ = writeln!(s, "  --font-family-{k}: {v};");
    }
    for (k, v) in &t.font_size {
        let _ = writeln!(s, "  --font-size-{k}: {v};");
    }
    for (k, v) in &t.leading {
        let _ = writeln!(s, "  --leading-{k}: {v};");
    }
    for (k, v) in &t.space {
        let _ = writeln!(s, "  --space-{k}: {v};");
    }
    for (k, v) in &t.layout {
        let _ = writeln!(s, "  --{k}: {v};");
    }
    let _ = writeln!(
        s,
        "  --link-underline-thickness: {};",
        t.links.underline_thickness
    );
    let _ = writeln!(
        s,
        "  --link-underline-offset: {};",
        t.links.underline_offset
    );
    let _ = writeln!(s, "}}");
    // The dark half, and the `[data-theme]` overrides that a colour-scheme
    // control drives, exist only when this build publishes both schemes. A
    // site forced to one serves that one to everybody: no media query to
    // follow, and nothing for an override to override.
    if scheme == crate::site::ColorScheme::Auto {
        let _ = writeln!(s, "@media (prefers-color-scheme: dark) {{");
        let _ = writeln!(s, "  :root:not([data-theme=\"light\"]) {{");
        let _ = writeln!(s, "    color-scheme: dark;");
        s.push_str(&scheme_block(&t.color.dark, "    "));
        let _ = writeln!(s, "  }}");
        let _ = writeln!(s, "}}");
        let _ = writeln!(s, ":root[data-theme=\"light\"] {{");
        let _ = writeln!(s, "  color-scheme: light;");
        s.push_str(&scheme_block(&t.color.light, "  "));
        let _ = writeln!(s, "}}");
        let _ = writeln!(s, ":root[data-theme=\"dark\"] {{");
        let _ = writeln!(s, "  color-scheme: dark;");
        s.push_str(&scheme_block(&t.color.dark, "  "));
        let _ = writeln!(s, "}}");
    }
    s
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Ratios measured independently. Reproducing them is the
    /// proof that this implementation is the same arithmetic as the audit.
    #[test]
    fn reproduces_the_audit() {
        let cases: &[(&str, &str, &str)] = &[
            ("#2680C2", "#FFFFFF", "4.24"),
            ("#2680C2", "#F0F4F8", "3.84"),
            ("#627D98", "#FFFFFF", "4.28"),
            ("#627D98", "#F0F4F8", "3.87"),
            ("#829AB1", "#FFFFFF", "2.91"),
            ("#FFA500", "#FFFFFF", "1.97"),
            ("#f9cb33", "#FFFFFF", "1.54"),
            ("#c0392b", "#f8d7da", "4.07"),
            ("#808080", "#FFFFFF", "3.95"),
            ("#005A9C", "#FFFFFF", "7.14"),
            ("#102A43", "#FFFFFF", "14.64"),
        ];
        for (fg, bg, expected) in cases {
            let got = contrast_ratio(Srgb::parse(fg).unwrap(), Srgb::parse(bg).unwrap());
            assert_eq!(&round2(got), expected, "{fg} on {bg}");
        }
    }

    #[test]
    fn black_on_white_is_21() {
        let r = contrast_ratio(
            Srgb::parse("#000000").unwrap(),
            Srgb::parse("#FFFFFF").unwrap(),
        );
        assert_eq!(round2(r), "21.00");
    }

    #[test]
    fn rejects_short_and_named_colours() {
        assert!(Srgb::parse("#fff").is_err());
        assert!(Srgb::parse("orange").is_err());
        assert!(Srgb::parse("#12345g").is_err());
    }
}

// ---------------------------------------------------------------------------
// Loading.
// ---------------------------------------------------------------------------

/// The token file the default theme ships. It is compiled in so that a build
/// needs no files on disk, and so that the gate and the emitted CSS cannot
/// drift from the values that were checked.
pub const DEFAULT_TOKENS: &str = include_str!("../assets/tokens.toml");

/// Parse the default token set.
pub fn default_tokens() -> anyhow::Result<Tokens> {
    toml::from_str(DEFAULT_TOKENS)
        .map_err(|e| anyhow::anyhow!("the built-in tokens.toml is invalid: {e}"))
}

/// The tokens a build should use: the defaults, with a theme's `tokens.toml`
/// merged over them if it has one.
///
/// **Merged, not replaced.** `Palette` requires all thirteen roles, so a
/// file that replaced the set would have to restate every value in both
/// schemes to change an accent -- 26 lines to change two, and 26 lines to
/// drift out of step with the next release. Merging means a brand theme is
/// the lines it actually changes, which is what the default file's own
/// header promises ("a brand theme that wants its own colour overrides two
/// lines per scheme and inherits everything else").
///
/// The merge is over TOML values rather than `Tokens`, because a partial
/// file cannot deserialise into `Tokens` at all: that is the same
/// all-roles-or-nothing rule doing its job, one layer too early.
///
/// Whatever comes out is handed to [`gate`] like any other tokens, so a
/// theme cannot ship a colour pair that fails WCAG contrast: the build
/// refuses. That gate existed before themes could bring their own colours
/// and was only ever pointed at this crate's own palette.
pub fn tokens_for(theme_dir: Option<&camino::Utf8Path>) -> anyhow::Result<Tokens> {
    let Some(path) = theme_dir
        .map(|d| d.join("tokens.toml"))
        .filter(|p| p.is_file())
    else {
        return default_tokens();
    };
    let text =
        std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    let overlay: toml::Value =
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {path}: {e}"))?;

    // A theme that declares a schema declares which one. A theme that
    // declares none is taken at its word for this version and will fail the
    // day the roles change, which is the point of the field.
    if let Some(schema) = overlay.get("schema").and_then(toml::Value::as_str)
        && schema != TOKENS_SCHEMA
    {
        anyhow::bail!(
            "{path} declares schema {schema:?}, and this build of iyo reads              {TOKENS_SCHEMA:?}"
        );
    }

    let mut merged: toml::Value = toml::from_str(DEFAULT_TOKENS)
        .map_err(|e| anyhow::anyhow!("the built-in tokens.toml is invalid: {e}"))?;
    merge_into(&mut merged, overlay);
    merged
        .try_into()
        .map_err(|e| anyhow::anyhow!("{path} is not a valid token set: {e}"))
}

/// Deep-merge `overlay` into `base`: tables recurse, everything else is
/// replaced. A theme setting `color.light.link` keeps the other twelve roles
/// and both schemes.
fn merge_into(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_into(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// Parse a theme's own token file.
pub fn load_tokens(path: &camino::Utf8Path) -> anyhow::Result<Tokens> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {path}: {e}"))
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn a_reference_with_no_definition_is_reported() {
        let sheet = "body { background: var(--surface); color: var(--color-text); }";
        let tokens = ":root { --color-surface: #fff; --color-text: #000; }";
        assert_eq!(undefined_tokens(&[sheet], tokens), vec!["--surface"]);
    }

    #[test]
    fn a_fallback_is_a_deliberate_default() {
        let sheet = "body { padding: var(--gap, 1rem); }";
        assert!(undefined_tokens(&[sheet], ":root {}").is_empty());
    }

    #[test]
    fn a_stylesheet_may_define_its_own() {
        let sheet = ":root { --gap: 1rem; } body { padding: var(--gap); }";
        assert!(undefined_tokens(&[sheet], ":root {}").is_empty());
    }

    #[test]
    fn the_bundled_theme_resolves_against_the_bundled_tokens() {
        let tokens = tokens_css(
            &default_tokens().expect("default tokens parse"),
            crate::site::ColorScheme::Auto,
        );
        let theme = include_str!("../assets/theme.css");
        assert_eq!(
            undefined_tokens(&[theme], &tokens),
            Vec::<String>::new(),
            "the default theme reads a token nothing defines"
        );
    }
}
