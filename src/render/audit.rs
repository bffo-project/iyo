//! Structural checks the binary runs over its own HTML, with no browser.
//!
//! These are the checks that catch the defects measured on the page this tool
//! replaces: a missing `lang`, duplicate ids, tables with no header cells, a
//! heading level skipped, an anchor with no text, a fragment that goes
//! nowhere. They run on every build, so a theme cannot regress them
//! silently. What needs a browser (contrast as rendered, focus order, reflow)
//! belongs in the CI suite, not here.
//!
//! The scanner is deliberately small: `iyo` generates the HTML it audits, so it
//! only has to understand the subset it produces, not arbitrary markup.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One tag occurrence in a document.
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub closing: bool,
    pub self_closing: bool,
    /// Byte offset of the `<`.
    pub start: usize,
    /// Byte offset just past the `>`.
    pub end: usize,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

/// Scan the tags of a document in order.
pub fn tags(html: &str) -> Vec<Tag> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // Skip comments and doctype.
        if html[i..].starts_with("<!--") {
            match html[i..].find("-->") {
                Some(j) => {
                    i += j + 3;
                    continue;
                }
                None => break,
            }
        }
        if html[i..].starts_with("<!") {
            match html[i..].find('>') {
                Some(j) => {
                    i += j + 1;
                    continue;
                }
                None => break,
            }
        }
        let Some(close_rel) = html[i..].find('>') else {
            break;
        };
        let end = i + close_rel + 1;
        let inner = &html[i + 1..end - 1];
        let inner = inner.strip_suffix('/').unwrap_or(inner);
        let self_closing = html[i + 1..end - 1].ends_with('/');
        let closing = inner.starts_with('/');
        let inner = inner.strip_prefix('/').unwrap_or(inner);

        let mut parts = inner.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default().to_ascii_lowercase();
        let attrs = parse_attrs(parts.next().unwrap_or_default());

        if !name.is_empty() {
            out.push(Tag {
                name,
                attrs,
                closing,
                self_closing,
                start: i,
                end,
            });
        }
        i = end;
    }
    out
}

fn parse_attrs(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let name = s[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = s[start..i.min(s.len())].to_owned();
                i += 1;
            } else {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = s[start..i].to_owned();
            }
        }
        out.insert(name, value);
    }
    out
}

/// The text content between a tag and its matching close, with markup removed.
fn text_after(html: &str, tags: &[Tag], index: usize) -> String {
    let open = &tags[index];
    let mut depth = 1usize;
    let mut end = html.len();
    for t in &tags[index + 1..] {
        if t.name == open.name {
            if t.closing {
                depth -= 1;
                if depth == 0 {
                    end = t.start;
                    break;
                }
            } else if !t.self_closing {
                depth += 1;
            }
        }
    }
    let slice = &html[open.end..end.max(open.end)];
    let mut text = String::new();
    let mut inside = false;
    for c in slice.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => text.push(c),
            _ => {}
        }
    }
    text.trim().to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Stable id, for example `a11y.duplicate-id`.
    pub rule: String,
    pub level: Level,
    pub message: String,
    pub page: String,
    /// The WCAG success criterion or the axe rule this stands in for.
    pub criterion: &'static str,
}

/// A minimal BCP 47 shape check: subtags of letters and digits, 1 to 8 long.
fn language_tag_looks_valid(tag: &str) -> bool {
    !tag.is_empty()
        && tag.split('-').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

/// Where an href on the page at `page_path` points, as a path from the site
/// root, or `None` when it leaves the site.
///
/// This has to understand the three forms the build emits: document-relative
/// on every ordinary page, root-relative on the 404 page, which a host answers
/// at whatever address was asked for, and absolute, which should no longer
/// appear in an `<a>` but is still resolved so the check does not go quiet if
/// one does.
fn resolve_internal(
    page_path: &str,
    href: &str,
    base_url: &str,
    base_path: &str,
) -> Option<String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.is_empty() {
        return None;
    }
    if !base_url.is_empty()
        && let Some(rest) = href.strip_prefix(base_url)
    {
        return Some(rest.trim_start_matches('/').to_owned());
    }
    if href.contains("://") || href.starts_with("//") || href.contains(':') {
        return None;
    }
    if href.starts_with('/') {
        // A root-absolute link is relative to the *server* root, and the
        // output directory is the site root, which sits below it whenever the
        // site is served from a path. 404.html is the page that needs such
        // links, because it is the one page whose address is not known in
        // advance. Resolving them against the output tree without removing
        // the base path first reported every one of them as broken.
        let rest = match base_path {
            "/" | "" => href.trim_start_matches('/'),
            prefix => href.strip_prefix(prefix).unwrap_or(
                // A link that does not carry the base path at all is outside
                // this site as deployed; leave it to resolve as it stands.
                href.trim_start_matches('/'),
            ),
        };
        return Some(rest.trim_start_matches('/').to_owned());
    }
    let dir = page_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    Some(segments.join("/"))
}

/// Audit one Markdown sibling.
///
/// The same two questions the HTML is asked, because the Markdown tree is a
/// navigable representation and not a byproduct: does any link name an origin,
/// which would tie the build to one deployment, and does every link go to a
/// file the build produced. A bare URL in the prose is a locator, not a link,
/// and is deliberately left absolute so a detached file still says where it
/// came from.
pub fn markdown(
    path: &str,
    body: &str,
    known: &BTreeSet<String>,
    base_url: &str,
    base_path: &str,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let Some(close) = rest.find(')') else { break };
        let target = &rest[..close];
        rest = &rest[close + 1..];
        if target.starts_with('#') {
            continue;
        }
        if !base_url.is_empty() && target.starts_with(base_url) {
            issues.push(Issue {
                rule: "markdown.absolute-link".to_owned(),
                level: Level::Error,
                message: format!("link to {target} is absolute; navigation must be relative"),
                page: path.to_owned(),
                criterion: "link integrity",
            });
            continue;
        }
        if target.contains("://") || target.contains(':') {
            continue;
        }
        if let Some(resolved) = resolve_internal(path, target, base_url, base_path)
            && !known.contains(&resolved)
        {
            issues.push(Issue {
                rule: "markdown.broken-link".to_owned(),
                level: Level::Warning,
                message: format!("link to {target} has no file in the output"),
                page: path.to_owned(),
                criterion: "link integrity",
            });
        }
    }
    issues
}

/// Whether a resolved target lies in a path this build does not own.
///
/// Entries are path prefixes. The one exception is `/`, which matches the
/// site's root document and nothing else: as a prefix it would exempt the
/// whole origin, and that is never what a publisher means by it. It is what
/// they mean when the documentation is mounted under `/ontology/` on a site
/// whose home page is a different program.
fn is_external(target: &str, external_paths: &[String]) -> bool {
    external_paths.iter().any(|p| {
        if p == "/" {
            target.is_empty() || target == "index.html"
        } else {
            target.starts_with(p.trim_start_matches('/'))
        }
    })
}

/// Audit one page. `known` is every path the build produced, used to check
/// that a link inside the site goes somewhere.
pub fn page(
    path: &str,
    html: &str,
    known: &BTreeSet<String>,
    base_url: &str,
    base_path: &str,
    external_paths: &[String],
) -> Vec<Issue> {
    let tags = tags(html);
    let mut issues = Vec::new();
    let mut add = |rule: &str, level: Level, criterion: &'static str, message: String| {
        issues.push(Issue {
            rule: rule.to_owned(),
            level,
            message,
            page: path.to_owned(),
            criterion,
        });
    };

    // --- document-level -------------------------------------------------
    match tags.iter().find(|t| t.name == "html" && !t.closing) {
        Some(html_tag) => match html_tag.attr("lang") {
            Some(lang) if language_tag_looks_valid(lang) => {}
            Some(lang) => add(
                "a11y.lang-invalid",
                Level::Error,
                "WCAG 3.1.1",
                format!("the html lang attribute {lang:?} is not a well-formed language tag"),
            ),
            None => add(
                "a11y.no-lang",
                Level::Error,
                "WCAG 3.1.1",
                "the html element has no lang attribute".to_owned(),
            ),
        },
        None => add(
            "a11y.no-html-element",
            Level::Error,
            "parsing",
            "no html element was found".to_owned(),
        ),
    }

    let head_bytes = html.len().min(1024);
    if !html[..head_bytes].to_ascii_lowercase().contains("charset") {
        add(
            "a11y.charset-late",
            Level::Error,
            "HTML validity",
            "no character encoding declaration in the first 1024 bytes".to_owned(),
        );
    }
    if !tags
        .iter()
        .any(|t| t.name == "meta" && t.attr("name") == Some("viewport"))
    {
        add(
            "a11y.no-viewport",
            Level::Error,
            "WCAG 1.4.10",
            "no viewport meta element, so the page cannot reflow".to_owned(),
        );
    }
    if !tags.iter().any(|t| t.name == "title" && !t.closing) {
        add(
            "a11y.no-title",
            Level::Error,
            "WCAG 2.4.2",
            "the page has no title element".to_owned(),
        );
    }

    // --- landmarks --------------------------------------------------------
    let count = |name: &str| tags.iter().filter(|t| t.name == name && !t.closing).count();
    for landmark in ["header", "main", "footer"] {
        if count(landmark) == 0 {
            add(
                "a11y.missing-landmark",
                Level::Error,
                "axe region, WCAG 1.3.1",
                format!("no {landmark} landmark"),
            );
        }
    }
    if count("main") > 1 {
        add(
            "a11y.multiple-main",
            Level::Error,
            "axe landmark-one-main",
            "more than one main landmark".to_owned(),
        );
    }
    let navs: Vec<&Tag> = tags
        .iter()
        .filter(|t| t.name == "nav" && !t.closing)
        .collect();
    if navs.len() > 1 {
        let named = navs
            .iter()
            .filter(|t| t.attr("aria-label").is_some() || t.attr("aria-labelledby").is_some())
            .count();
        if named != navs.len() {
            add(
                "a11y.unnamed-nav",
                Level::Error,
                "axe landmark-unique",
                "a page with several nav landmarks must name each one".to_owned(),
            );
        }
    }

    // --- headings ---------------------------------------------------------
    let mut levels: Vec<u8> = Vec::new();
    for t in &tags {
        if t.closing || t.name.len() != 2 || !t.name.starts_with('h') {
            continue;
        }
        if let Some(d) = t.name[1..]
            .parse::<u8>()
            .ok()
            .filter(|d| (1..=6).contains(d))
        {
            levels.push(d);
        }
    }
    let h1s = levels.iter().filter(|l| **l == 1).count();
    if h1s == 0 {
        add(
            "a11y.no-h1",
            Level::Error,
            "WCAG 1.3.1",
            "the page has no h1".to_owned(),
        );
    } else if h1s > 1 {
        add(
            "a11y.multiple-h1",
            Level::Warning,
            "axe page-has-heading-one",
            format!("the page has {h1s} h1 elements"),
        );
    }
    for pair in levels.windows(2) {
        if pair[1] > pair[0] + 1 {
            add(
                "a11y.heading-skip",
                Level::Error,
                "axe heading-order, WCAG 1.3.1",
                format!("heading level jumps from h{} to h{}", pair[0], pair[1]),
            );
            break;
        }
    }

    // --- ids ---------------------------------------------------------------
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut fragments: BTreeSet<&str> = BTreeSet::new();
    for t in &tags {
        if let Some(id) = t.attr("id") {
            if id.is_empty() {
                add(
                    "a11y.empty-id",
                    Level::Error,
                    "HTML validity",
                    format!("an {} element has an empty id", t.name),
                );
            } else if id.contains(':') || id.contains(' ') {
                add(
                    "a11y.invalid-id",
                    Level::Error,
                    "HTML validity, html-validate valid-id",
                    format!("id {id:?} contains a character that breaks a fragment reference"),
                );
            }
            if !seen.insert(id) {
                add(
                    "a11y.duplicate-id",
                    Level::Error,
                    "axe duplicate-id, WCAG 4.1.1",
                    format!("id {id:?} appears more than once"),
                );
            }
        }
    }
    for t in &tags {
        if t.name == "a"
            && let Some(href) = t.attr("href")
            && let Some(frag) = href.strip_prefix('#')
        {
            fragments.insert(frag);
        }
    }
    for frag in &fragments {
        if !seen.contains(frag) {
            add(
                "a11y.dangling-fragment",
                Level::Error,
                "WCAG 2.4.4",
                format!("link to #{frag} but nothing on the page has that id"),
            );
        }
    }

    // --- links -------------------------------------------------------------
    for (i, t) in tags.iter().enumerate() {
        if t.name != "a" || t.closing {
            continue;
        }
        let text = text_after(html, &tags, i);
        let labelled = t.attr("aria-label").is_some_and(|l| !l.trim().is_empty())
            || t.attr("aria-labelledby").is_some()
            || t.attr("title").is_some_and(|l| !l.trim().is_empty());
        if text.is_empty() && !labelled {
            add(
                "a11y.empty-link",
                Level::Error,
                "axe link-name, WCAG 2.4.4",
                format!(
                    "a link to {} has no text",
                    t.attr("href").unwrap_or("(no href)")
                ),
            );
        }
        // Navigation is relative so that one build serves from any mount. An
        // absolute one still resolves on the origin it names, and nowhere else,
        // so it is a defect that only shows up in a preview or on a staging
        // host. `canonical` and `cite-as` are identity, not navigation, and are
        // `<link>` elements rather than `<a>`, so they never reach here.
        //
        // Only when the target is a page *this build wrote*. A vocabulary can
        // share an origin with an application that is not this build --
        // BFFO's documentation is served at /ontology/ and /vocabulary/ on a
        // site whose /formats/ and /about/ are a different program -- and a
        // link to that neighbour is external in every sense but the
        // hostname. Making it relative would point it at a page this tree
        // does not have. A mistyped internal link is still caught, by
        // `a11y.broken-internal-link` below, which resolves the same href
        // against the same set.
        if let Some(href) = t.attr("href")
            && !base_url.is_empty()
            && href.starts_with(base_url)
            && resolve_internal(path, href, base_url, base_path).is_some_and(|target| {
                // A path the publisher has declared someone else's is not
                // this build's to make relative, even where this build
                // happens to write a file at the same address.
                !is_external(&target, external_paths)
                    && [
                        target.clone(),
                        format!("{target}.html"),
                        format!("{target}index.html"),
                        format!("{target}/index.html"),
                    ]
                    .iter()
                    .any(|c| known.contains(c))
            })
        {
            add(
                "html.absolute-nav-link",
                Level::Error,
                "link integrity",
                format!("link to {href} is absolute; navigation must be relative"),
            );
        }
        // A link inside this site should go somewhere the build produced --
        // unless it points into a path this build does not own, which is a
        // fact only the configuration knows (`site.external_paths`).
        if let Some(href) = t.attr("href")
            && let Some(target) = resolve_internal(path, href, base_url, base_path)
            && !is_external(&target, external_paths)
        {
            let candidates = [
                target.clone(),
                format!("{target}.html"),
                format!("{target}index.html"),
                format!("{target}/index.html"),
            ];
            if !candidates.iter().any(|c| known.contains(c)) {
                add(
                    "a11y.broken-internal-link",
                    Level::Warning,
                    "link integrity",
                    format!("link to {href} has no file in the output"),
                );
            }
        }
    }

    // --- tables ------------------------------------------------------------
    let table_indices: Vec<usize> = tags
        .iter()
        .enumerate()
        .filter(|(_, t)| t.name == "table" && !t.closing)
        .map(|(i, _)| i)
        .collect();
    for i in table_indices {
        // Find the extent of this table.
        let mut depth = 1usize;
        let mut end_index = tags.len();
        for (j, t) in tags.iter().enumerate().skip(i + 1) {
            if t.name == "table" {
                if t.closing {
                    depth -= 1;
                    if depth == 0 {
                        end_index = j;
                        break;
                    }
                } else {
                    depth += 1;
                }
            }
        }
        let inner = &tags[i..end_index];
        if !inner.iter().any(|t| t.name == "caption" && !t.closing) {
            add(
                "a11y.table-no-caption",
                Level::Warning,
                "WCAG 1.3.1, technique H39",
                "a table has no caption".to_owned(),
            );
        }
        let headers: Vec<&Tag> = inner
            .iter()
            .filter(|t| t.name == "th" && !t.closing)
            .collect();
        if headers.is_empty() {
            add(
                "a11y.table-no-headers",
                Level::Error,
                "axe td-has-header, WCAG 1.3.1",
                "a table has no header cells".to_owned(),
            );
        } else if headers.iter().any(|t| t.attr("scope").is_none()) {
            add(
                "a11y.th-no-scope",
                Level::Warning,
                "WCAG 1.3.1, technique H63",
                "a header cell has no scope attribute".to_owned(),
            );
        }
    }

    // --- markup that reached the page as text --------------------------------
    //
    // A template that concatenates tags around a value and marks the result
    // safe does not work in Jinja: `~` binds looser than a filter, so only the
    // last fragment is marked and the row is escaped into visible markup. The
    // result is valid HTML, so no validator and no browser-based checker sees
    // it; only a reader does. Eight fact rows shipped that way before this
    // rule existed.
    {
        let mut seen = BTreeSet::new();
        for opener in [
            "&lt;a ",
            "&lt;code",
            "&lt;span",
            "&lt;p&gt;",
            "&lt;div",
            "&lt;strong",
        ] {
            // Every occurrence, not the first: a page whose examples quote
            // markup would otherwise mask a real leak further down.
            for (at, _) in html.match_indices(opener) {
                // Inside a code block the text is a quoted example, not a leak.
                let in_example =
                    html[..at].matches("<pre").count() > html[..at].matches("</pre>").count();
                if !in_example && seen.insert(opener) {
                    add(
                        "html.escaped-markup",
                        Level::Error,
                        "template correctness",
                        format!(
                            "{opener:?} appears as text, so a template escaped markup it meant to emit"
                        ),
                    );
                    break;
                }
            }
        }
    }

    // --- language of parts --------------------------------------------------
    for t in &tags {
        if let Some(lang) = t.attr("lang")
            && t.name != "html"
            && !language_tag_looks_valid(lang)
        {
            add(
                "a11y.lang-part-invalid",
                Level::Error,
                "WCAG 3.1.2",
                format!("lang={lang:?} on a {} element is not well formed", t.name),
            );
        }
    }

    issues.sort_by(|a, b| (b.level, &a.rule, &a.message).cmp(&(a.level, &b.rule, &b.message)));
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_markup_in_text_is_an_error() {
        let bad = concat!(
            "<html lang=\"en\"><body><main><h1>t</h1>",
            "<dl><div><dt>CURIE</dt><dd>&lt;code&gt;ex:a&lt;/code&gt;</dd></div></dl>",
            "</main></body></html>"
        );
        assert!(audit(bad).iter().any(|m| m.contains("html.escaped-markup")));
    }

    #[test]
    fn an_example_earlier_on_the_page_does_not_mask_a_real_leak() {
        let bad = concat!(
            "<html lang=\"en\"><body><main><h1>t</h1>",
            "<pre><code>&lt;code&gt;quoted&lt;/code&gt;</code></pre>",
            "<dl><div><dt>CURIE</dt><dd>&lt;code&gt;ex:a&lt;/code&gt;</dd></div></dl>",
            "</main></body></html>"
        );
        assert!(audit(bad).iter().any(|m| m.contains("html.escaped-markup")));
    }

    #[test]
    fn escaped_markup_inside_a_code_block_is_an_example() {
        let ok = concat!(
            "<html lang=\"en\"><body><main><h1>t</h1>",
            "<pre><code>&lt;code&gt;ex:a&lt;/code&gt;</code></pre>",
            "</main></body></html>"
        );
        assert!(!audit(ok).iter().any(|m| m.contains("html.escaped-markup")));
    }

    fn audit(html: &str) -> Vec<String> {
        // A real build always writes an index, and the link check now resolves
        // relative and root-relative hrefs, so an empty set would report the
        // fixture's own `<a href="/">` as broken.
        let known = BTreeSet::from(["index.html".to_owned()]);
        page("test.html", html, &known, "https://example.org/", "/", &[])
            .into_iter()
            .map(|i| i.rule)
            .collect()
    }

    #[test]
    fn markdown_links_are_checked_and_bare_urls_are_left_alone() {
        let known = BTreeSet::from(["vocab/Widget.md".to_owned()]);
        let rules = |body: &str| -> Vec<String> {
            markdown("vocab/Thing.md", body, &known, "https://example.org/", "/")
                .into_iter()
                .map(|i| i.rule)
                .collect()
        };

        // A bare URL states where something is. That is the whole reason a
        // detached file still says what it is, so it must not be flagged.
        assert!(rules("- IRI: `https://example.org/vocab/Thing`\n").is_empty());
        assert!(rules("Canonical page: https://example.org/vocab/Thing\n").is_empty());
        // A link that goes to a file the build wrote is fine.
        assert!(rules("see [Widget](Widget.md)\n").is_empty());
        // An off-site link is not ours to check.
        assert!(rules("see [SKOS](https://www.w3.org/TR/skos-reference/)\n").is_empty());

        assert_eq!(
            rules("see [Widget](https://example.org/vocab/Widget.md)\n"),
            vec!["markdown.absolute-link"]
        );
        assert_eq!(
            rules("see [Gone](../vocab/Gone.md)\n"),
            vec!["markdown.broken-link"]
        );
    }

    #[test]
    fn an_absolute_navigation_link_is_an_error() {
        // A page this build wrote, linked absolutely: the defect. It
        // resolves on the origin it names and nowhere else, so it breaks in
        // a preview and on a staging host and not on the author's.
        let html = GOOD.replace(
            r#"<a href="/">Home</a>"#,
            r#"<a href="https://example.org/index.html">Home</a>"#,
        );
        assert!(audit(&html).contains(&"html.absolute-nav-link".to_owned()));
    }

    /// The same shape of link, to a path this build did not write: another
    /// application sharing the hostname. Making it relative would point it
    /// at a page this tree does not have.
    ///
    /// BFFO is the case: its documentation is served at `/ontology/` and
    /// `/vocabulary/` on a site whose `/formats/` and `/about/` are a
    /// separate program. Before this distinction, six such links in a theme's
    /// header produced 233 errors and 2,097 warnings and the build refused.
    #[test]
    fn an_absolute_link_to_a_neighbour_on_the_same_origin_is_not() {
        let html = GOOD.replace(
            r#"<a href="/">Home</a>"#,
            r#"<a href="https://example.org/formats/">Browse</a>"#,
        );
        let issues = audit(&html);
        assert!(
            !issues.contains(&"html.absolute-nav-link".to_owned()),
            "a link to a neighbouring application was called a defect: {issues:?}"
        );
    }

    /// `/` in `site.external_paths` means the site's root document and not
    /// the whole origin. As a prefix it would exempt every page, which is
    /// never what a publisher means by it; what they mean is that the home
    /// page belongs to the application they are mounted beside.
    #[test]
    fn declaring_the_root_external_exempts_the_root_and_nothing_else() {
        let root_link = GOOD.replace(
            r#"<a href="/">Home</a>"#,
            r#"<a href="https://example.org/">BFFO</a>"#,
        );
        let known = BTreeSet::from(["index.html".to_owned(), "vocab/Widget.html".to_owned()]);
        let root = "/".to_owned();

        // Without the declaration: an absolute link to a page this build
        // writes, which is the defect the rule is for.
        let undeclared = page(
            "test.html",
            &root_link,
            &known,
            "https://example.org/",
            "/",
            &[],
        );
        assert!(
            undeclared
                .iter()
                .any(|i| i.rule == "html.absolute-nav-link"),
            "{undeclared:?}"
        );

        // With it: the home page is the neighbour's, and linking to it
        // absolutely is the only way to reach it.
        let declared = page(
            "test.html",
            &root_link,
            &known,
            "https://example.org/",
            "/",
            std::slice::from_ref(&root),
        );
        assert!(
            !declared.iter().any(|i| i.rule == "html.absolute-nav-link"),
            "{declared:?}"
        );

        // And it exempts nothing else: a term page linked absolutely is
        // still the defect, with `/` declared.
        let term_link = GOOD.replace(
            r#"<a href="/">Home</a>"#,
            r#"<a href="https://example.org/vocab/Widget.html">Widget</a>"#,
        );
        let still = page(
            "test.html",
            &term_link,
            &known,
            "https://example.org/",
            "/",
            std::slice::from_ref(&root),
        );
        assert!(
            still.iter().any(|i| i.rule == "html.absolute-nav-link"),
            "declaring / external exempted a term page too: {still:?}"
        );
    }

    /// And it is still reported as going nowhere, unless the configuration
    /// says that path belongs to someone else. A typo is indistinguishable
    /// from a neighbour by URL alone, so one of them has to be declared.
    #[test]
    fn a_neighbour_is_only_not_broken_once_it_is_declared() {
        let html = GOOD.replace(
            r#"<a href="/">Home</a>"#,
            r#"<a href="https://example.org/formats/">Browse</a>"#,
        );
        let known = BTreeSet::from(["index.html".to_owned()]);
        let undeclared = page("test.html", &html, &known, "https://example.org/", "/", &[]);
        assert!(
            undeclared
                .iter()
                .any(|i| i.rule == "a11y.broken-internal-link"),
            "an undeclared path that this build did not write should still \
             read as broken: it is how a mistyped term is caught"
        );

        let declared = page(
            "test.html",
            &html,
            &known,
            "https://example.org/",
            "/",
            &["/formats/".to_owned()],
        );
        assert!(
            !declared
                .iter()
                .any(|i| i.rule == "a11y.broken-internal-link"),
            "site.external_paths did not exempt the neighbour"
        );
    }

    #[test]
    fn a_relative_link_to_nothing_is_caught() {
        let html = GOOD.replace(r#"<a href="/">Home</a>"#, r#"<a href="../gone">Home</a>"#);
        assert!(audit(&html).contains(&"a11y.broken-internal-link".to_owned()));
    }

    #[test]
    fn hrefs_resolve_against_the_page_they_are_on() {
        let base = "https://example.org/";
        // Document-relative, from two directories down.
        assert_eq!(
            resolve_internal(
                "vocabulary/category/alignment.html",
                "../../ontology/",
                base,
                "/"
            ),
            Some("ontology".to_owned())
        );
        // A sibling term.
        assert_eq!(
            resolve_internal("ontology/Format.html", "baseFormat", base, "/"),
            Some("ontology/baseFormat".to_owned())
        );
        // Root-relative, which is what the 404 page emits.
        assert_eq!(
            resolve_internal("404.html", "/vocabulary/category/", base, "/"),
            Some("vocabulary/category/".to_owned())
        );
        // Absolute, still resolved so the check cannot go quiet.
        assert_eq!(
            resolve_internal("ontology/Format.html", "https://example.org/a/b", base, "/"),
            Some("a/b".to_owned())
        );
        // Off-site and in-page links are not ours to check.
        assert_eq!(
            resolve_internal("ontology/Format.html", "https://w3.org/x", base, "/"),
            None
        );
        assert_eq!(
            resolve_internal("ontology/Format.html", "#iyo-facts", base, "/"),
            None
        );
    }

    const GOOD: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>A page</title></head>
<body><a class="skip" href="#main">Skip to content</a>
<header><nav aria-label="Site"><a href="/">Home</a></nav></header>
<main id="main"><h1>Title</h1><h2>Section</h2>
<table><caption>Facts</caption><thead><tr><th scope="col">Field</th></tr></thead>
<tbody><tr><td>value</td></tr></tbody></table>
<p><span lang="ja">アラインメント</span></p></main>
<footer><p>Footer</p></footer></body></html>"##;

    #[test]
    fn a_correct_page_has_no_issues() {
        assert_eq!(audit(GOOD), Vec::<String>::new());
    }

    #[test]
    fn missing_lang_is_an_error() {
        let html = GOOD.replace("<html lang=\"en\">", "<html>");
        assert!(audit(&html).contains(&"a11y.no-lang".to_owned()));
    }

    #[test]
    fn duplicate_and_invalid_ids_are_caught() {
        let html = GOOD.replace("<h2>Section</h2>", "<h2 id=\"main\">Section</h2>");
        assert!(audit(&html).contains(&"a11y.duplicate-id".to_owned()));

        let html = GOOD.replace("<h2>Section</h2>", "<h2 id=\"dcterms:title\">Section</h2>");
        assert!(
            audit(&html).contains(&"a11y.invalid-id".to_owned()),
            "a colon in an id breaks fragment references and fails html-validate"
        );
    }

    #[test]
    fn heading_skips_are_caught() {
        let html = GOOD.replace("<h2>Section</h2>", "<h4>Section</h4>");
        assert!(audit(&html).contains(&"a11y.heading-skip".to_owned()));
    }

    #[test]
    fn a_table_without_headers_is_an_error() {
        let html = GOOD.replace(
            "<thead><tr><th scope=\"col\">Field</th></tr></thead>",
            "<thead><tr><td>Field</td></tr></thead>",
        );
        let rules = audit(&html);
        assert!(rules.contains(&"a11y.table-no-headers".to_owned()));
    }

    #[test]
    fn a_dangling_fragment_is_caught() {
        let html = GOOD.replace("href=\"#main\"", "href=\"#nowhere\"");
        assert!(audit(&html).contains(&"a11y.dangling-fragment".to_owned()));
    }

    #[test]
    fn an_empty_link_is_caught() {
        let html = GOOD.replace("<a href=\"/\">Home</a>", "<a href=\"/\"></a>");
        assert!(audit(&html).contains(&"a11y.empty-link".to_owned()));
    }

    #[test]
    fn attributes_parse_with_and_without_quotes() {
        let t = tags(r##"<a href='/x' data-n=3 hidden>text</a>"##);
        assert_eq!(t[0].attr("href"), Some("/x"));
        assert_eq!(t[0].attr("data-n"), Some("3"));
        assert_eq!(t[0].attr("hidden"), Some(""));
    }
}
