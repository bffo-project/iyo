# Theming

[README](../README.md) · [cli](cli.md) · [rules](rules.md) · **theming** · [output-convention](output-convention.md) · [ci](ci.md)

What a theme may override, what each template receives, every design token, and
what the contrast gate requires.

## The override surface: one directory, four kinds of file

A theme is a directory passed as `--theme DIR` to `iyo build` (env `IYO_THEME`). It is an overlay applied file by file over a default theme compiled into the binary, so a theme ships only the files it changes.

| Path inside the theme dir | Overrides | Mechanism |
|---|---|---|
| `templates/<name>.html.jinja` | the HTML template of that name | rendered by minijinja with a context |
| `templates/partials/<name>.html.jinja` | that partial | same; subdirectories addressed with `/` |
| `assets/**` (any file, any depth) | `assets/<same relative path>` in the output | copied byte for byte, no templating |
| `tokens.toml` | the design tokens | parsed as TOML and deep-merged over the built-in tokens |
| `pdf/spec.typ` | the Typst source of the PDF | copied verbatim, NOT templated |

Nothing else in the directory is read. There is no theme manifest file.

Template lookup order: the theme's `templates/` directory first (`minijinja::path_loader`), then the embedded default of the same name. A theme with no `templates/` directory is valid.

Only HTML is themeable. The Markdown, Turtle, JSON-LD, `llms.txt`, `terms.json` and `manifest.json` writers use no template engine.

Directory validation, run before anything is loaded:

| Condition | Result |
|---|---|
| `--theme` path is not a directory | error, exit 3: "<dir> is not a directory, so there is no theme to read" |
| directory holds none of `templates/`, `assets/`, `tokens.toml`, `pdf/spec.typ` | error, exit 3: "<dir> holds nothing a theme can override" |

A theme that only changes colours needs nothing beside its `tokens.toml`.

There is no `theme` key in `iyo.toml`. `[site] theme = "x"` is rejected by the config parser (unknown key).

## Every template a theme may override

Eight files. A theme may also add templates of its own and reference them from its overrides.

| Template | Written for | Requested by | Context variables |
|---|---|---|---|
| `base.html.jinja` | the layout every page extends | `{% extends %}` from the four page templates | whatever the extending page received |
| `index.html.jinja` | the site index (`index.html`) | Rust, by name | `site`, `page` |
| `404.html.jinja` | the not-found page (`404.html`) | Rust, by name | `site`, `page` |
| `document.html.jinja` | one namespace's document page | Rust, by name | `site`, `document`, `page` |
| `term.html.jinja` | one term page | Rust, by name | `site`, `term`, `page` |
| `partials/facts.html.jinja` | macros `refs`, `fact`, `mono`, `link`, `fields`, `releases` | `{% import %}` from index/document/term | macro arguments only |
| `partials/theme-switch.html.jinja` | the pre-paint head script | `{% include %}` from `base` when `site.theme_switch` | inherits the page context |
| `partials/theme-switch-button.html.jinja` | the header button and its script | `{% include %}` from `base` when `site.theme_switch` | inherits the page context |

Blocks `base.html.jinja` defines, for a theme that overrides a page template but keeps the layout: `title`, `meta`, `links`, `structured_data`, `contents`, `main`.

Landmarks the default `base` guarantees, because the build's own HTML auditor errors without them: `<meta charset>` first in head, `<meta name="viewport">` second, a skip link as the first element of body targeting `main[tabindex="-1"]`, a named `nav`, exactly one `main`, and all content inside `header`/`nav`/`main`/`footer`.

## Template context: `page`

Per-page values that are not part of the vocabulary. Serialised from `Page` in `src/render/html.rs:218-243`.

| Variable | Type | Notes |
|---|---|---|
| `page.kind` | string | one of `document`, `term`, `index`, `404` |
| `page.title` | string | built to a 70-character budget: `label (CURIE) — site`, dropping the CURIE then the site name then truncating with `…` |
| `page.assets` | string | href prefix for the asset directory, e.g. `../../assets/` |
| `page.root` | string | path from this page back to the site root; `""` at depth 0, else `../` repeated per `/` in the path |
| `page.llms_url` | string | absolute URL of the relevant `llms.txt` |
| `page.license` | string, may be absent | vocabulary licence IRI |
| `page.license_label` | string, may be absent | human name, e.g. `CC0 1.0`, `CC BY 4.0` |
| `page.collection_url` | string, may be absent | term pages only: the document page the term belongs to |
| `page.cite_as` | bool | whether to emit `rel="cite-as"`; from `site.cite_as`, always `false` on the index and 404 |
| `page.jsonld` | string | pre-serialised JSON-LD block; use with `| safe` |
| `page.banner` | object, may be absent | `.title` (string) and `.paragraphs` (list of strings) |
| `page.hierarchy_html` | string | pre-rendered `<ul class="tree">`; empty except on a document page with a hierarchy; use with `| safe` |

`page.jsonld` and `page.hierarchy_html` are built in Rust: escaping HTML inside a JSON string corrupts it, and the hierarchy's hrefs are relativised before they reach the template, so they must not pass through `| rel` again.

On `404.html` only, `page.assets` and `page.root` are root-relative (`/assets/`, `/`) rather than document-relative, because a host answers 404 at whatever path was asked for. The root comes from the path component of `--base-url` unless `--base-path` overrides it.

## Template context: `site`

Site-wide values every page gets. Serialised from `SiteView` in `src/render/view.rs:336-370`.

| Variable | Type | Source |
|---|---|---|
| `site.base_url` | string | `--base-url` / `[site] base_url` |
| `site.lang` | string | `[site] lang` (default `en`) |
| `site.title` | string | `[site] title`, else the root document's label, else `Vocabulary` |
| `site.description` | string, may be absent | the root document's description, whitespace-collapsed |
| `site.generator` | string | the crate name |
| `site.generator_version` | string | the crate version |
| `site.doc_license` | string, may be absent | `[site] doc_license` (documentation licence, distinct from the RDF's `dcterms:license`) |
| `site.doc_license_label` | string, may be absent | human name of the above; a link needs text |
| `site.theme_switch` | bool | `--theme-switch` / `[site] theme_switch` |
| `site.namespaces` | list | each with `.iri`, `.prefix` (may be absent), `.url`, `.llms_url`, `.title` (may be absent), `.term_count` |
| `site.prefixes` | list of 2-tuples | prefix, namespace IRI; indexed `p.0` / `p.1` in a template |
| `site.llms_url` | string | `<base>llms.txt` |
| `site.terms_url` | string | `<base>terms.json` |
| `site.manifest_url` | string | `<base>manifest.json` |
| `site.release_ttl_url` | string | `<base>release.ttl` |
| `site.term_count` | integer | local terms in the release |
| `site.document_count` | integer | documents in the release |

A theme that supplies its own `base.html.jinja` chooses whether to include the two theme-switch partials. `site.theme_switch` is the flag to branch on.

## Template context: `term`

Present on `term.html.jinja` only. From `TermView` in `src/render/view.rs:176-231`. Fields marked "may be absent" are `Option` with `skip_serializing_if`. Every list is always present and may be empty.

Identity and text: `term.iri`, `term.local_name`, `term.curie` (may be absent), `term.anchor`, `term.kind` (human wording, e.g. "object property"), `term.kind_id` (machine token for a CSS class), `term.section` (the heading its kind groups under), `term.label`, `term.labels`, `term.alt_labels`, `term.definition` (may be absent), `term.definitions`, `term.comments`, `term.notes`, `term.examples`, `term.see_also` (list of strings).

Relations: `term.super_terms`, `term.sub_terms`, `term.equivalent`, `term.disjoint_with`, `term.replaced_by` (all lists of `Ref`); `term.mappings` (each `.relation`, `.relation_label`, `.target` as a `Ref`); `term.defined_in` (a `Ref`, may be absent).

Grouped sub-objects, always present:
- `term.property`: `.domain`, `.range`, `.domain_includes`, `.range_includes`, `.inverse_of` (lists of `Ref`), `.characteristics` (list of strings)
- `term.concept`: `.in_scheme`, `.top_concept_of`, `.broader`, `.narrower`, `.related` (lists of `Ref`), `.notation` (may be absent)

Status and the rest: `term.deprecated` (bool), `term.status` (may be absent), `term.residue` (list of `.predicate`, `.predicate_label`, `.object`, `.object_is_iri`, `.lang`), `term.turtle` (the term's own Turtle, ready for a code block), `term.url` (identity; what `canonical` and `cite-as` say), `term.doc_url` (where a navigation link should point; differs from `url` only under `--link-style file`), `term.siblings` (list of `.rel`, `.media_type`, `.label`, `.url`), `term.shapes` (list of `ShapeView`), `term.constraints` (list of `FieldView`).

`Ref` (every term reference): `.iri`, `.label`, `.curie` (may be absent), `.url` (may be absent, present only when this release publishes the term), `.anchor` (may be absent), `.foreign` (bool).

`Value` (every literal): `.value`, `.lang` (may be absent), `.source` (predicate IRI), `.source_label` (predicate CURIE).

`ShapeView`: `.shape` (a `Ref`), `.targeting` (string), `.closed` (may be absent), `.fields` (list of `FieldView`), `.required_count`.

`FieldView`: `.name`, `.property` (a `Ref`, may be absent), `.path`, `.cardinality` (may be absent), `.required` (bool), `.value_type`, `.in_scheme` (list of `Ref`), `.values` (list of strings), `.pattern` (may be absent), `.description` (may be absent), `.shape` (a `Ref`), `.deactivated` (bool).

Templates never resolve an IRI, pick a language or compute a URL. Every reference arrives pre-resolved.

## Template context: `document`

Present on `document.html.jinja` only. From `DocumentView` in `src/render/view.rs:252-320`.

Identity and description: `document.iri`, `document.kind`, `document.profile`, `document.title`, `document.description` (a `Value`, may be absent), `document.abstract_paragraphs` (list of strings), `document.comment` (list of `Value`), `document.namespace`, `document.prefix` (may be absent).

Versioning: `document.version` (may be absent), `document.version_iri` (may be absent), `document.versions` (list of `.segment`, `.url`, `.source`, `.is_version_iri`), `document.versions_url` (may be absent).

Provenance: `document.status` (may be absent), `document.status_iri` (may be absent), `document.license` (may be absent), `document.doc_license` (may be absent), `document.creators`, `document.publishers`, `document.contributors` (lists of `.name`/`.iri`/`.kind`, each may be absent), `document.created`, `document.modified`, `document.issued`, `document.citation` (all may be absent), `document.see_also` (list of strings), `document.has_part` (list of `Ref`).

Changes: `document.changes_url` (may be absent), `document.change_count`, `document.breaking_count`, `document.removed` (list of `.label`, `.iri`, `.anchor`, `.detail`).

Content: `document.sections` (list of `.id`, `.title`, `.terms`, each term a full `TermView`), `document.reused` (list of `TermView`), `document.hierarchy` (nested `.term`/`.children`; use `page.hierarchy_html` to render it rather than walking it), `document.term_count`.

URLs: `document.url`, `document.llms_url`, `document.pdf_url` (may be absent), `document.siblings`.

## Template engine: filters and settings

minijinja 2.24.0 with the `loader` feature. Four custom filters are registered. The rest are minijinja's built-ins (`default`, `join`, `length`, `safe`, `upper`, and so on).

| Filter | Argument | Behaviour |
|---|---|---|
| `url` | a string | marks the value safe (unescaped) unless it contains `"`, `<`, `>`, a space or a newline, in which case it is escaped normally. Needed because HTML autoescape also escapes `/`, which would turn `https://…` into `https:&#x2f;&#x2f;…`. Use for absolute URLs that must stay absolute: `rel="canonical"`, `rel="cite-as"`, `rel="license"`, media types |
| `rel` | a string | rewrites an absolute URL inside the site into an href relative to the current page (using `site.base_url` and `page.root`), then applies `url`. A URL outside the site, or any URL when `base_url` is empty, passes through unchanged. Use for everything a reader or crawler follows |
| `lang_attr` | a language tag or nothing | emits ` lang="ja"` when the tag is non-empty and contains only ASCII alphanumerics and `-`; emits nothing otherwise. Written to be applied directly inside a start tag: `<p{{ d.lang | lang_attr }}>` |
| `oneline` | a string | collapses all whitespace runs to single spaces. For `meta` descriptions and one-line index entries |

Environment settings that affect authoring:
- Autoescape is minijinja's default callback. `*.html.jinja` strips `.jinja`, sees `.html`, and gets HTML autoescaping. A template named without `.html` before `.jinja` would not be escaped.
- `set_debug(true)` is forced on even in release builds, so a template error reports a line and an excerpt.
- `set_keep_trailing_newline(true)`.

Authoring note carried in the default partial: concatenating tags around a value with `~` and applying `| safe` at the end does not work, because `~` binds looser than a filter, so only the closing tag is marked safe. Use a macro instead, which returns markup by construction and still escapes the value it interpolates.

## Assets: what is copied, and the one refusal

Three sources feed `assets/` in the output, in this order:

1. **Embedded defaults.** Only non-template files ending in `.css` are copied out of the binary. In practice that is `assets/theme.css`, the default stylesheet. The embedded `tokens.toml` and `pdf/spec.typ` are inputs and are never copied to `assets/`.
2. **`assets/tokens.css`**, generated from the merged tokens on every build (see the tokens.css section).
3. **The theme's `assets/`**, walked recursively; each file is written to `assets/<same relative path>`, replacing an embedded file of the same name.

Text and binary are told apart by whether the bytes are valid UTF-8, not by extension, so a `logo.svg` and a `fonts/Inter.woff2` both copy correctly.

**Refusal:** a theme that ships `assets/tokens.css` fails the build with "…would be overwritten: tokens.css is generated from tokens.toml so the custom properties and the contrast-checked values cannot drift. Put the values in the theme's tokens.toml instead." This is an error, not a silent skip.

**Replacement, not merge.** A theme's `assets/theme.css` replaces the bundled stylesheet entirely. It does not cascade after it. A theme that wants to keep the default look and add to it should start from a copy of the bundled `assets/theme.css`, or ship its own extra `.css` file under a different name and reference it from an overridden `base.html.jinja`.

**Undefined-token check.** After assets are assembled, every `var(--name)` reference in every `.css` file in the output except `tokens.css` must resolve to a custom property defined either in `tokens.css` or in one of the stylesheets. A reference with no definition and no fallback fails the build: "the theme reads N custom properties that tokens.css does not define: …". A `var(--x, fallback)` reference is exempt. An unresolvable `var()` makes the declaration invalid at computed-value time, so the page silently keeps browser defaults while the contrast gate still reports success.

## tokens.toml: file shape and the deep merge

`<theme>/tokens.toml` (at the theme root, not under `assets/`). It is deep-merged over the built-in token file: tables recurse, every other value is replaced. A theme that sets `color.light.link` keeps the other twelve roles and both schemes.

Accepted top-level keys, exactly (the parser rejects anything else by name):
`schema`, `meta`, `color`, `links`, `font-family`, `font-size`, `leading`, `space`, `layout`.

| Key | Shape | Notes |
|---|---|---|
| `schema` | string | `"iyo.tokens/1"` in this build. A theme that declares a different value is rejected: "…declares schema \"x\", and this build of iyo reads \"iyo.tokens/1\"". A theme that declares none is taken at its word |
| `meta` | table | `name` and `version` required in the merged result (inherited if omitted); `description` and `license` optional. `name` and `version` appear in a comment at the top of the generated `tokens.css` |
| `color.light` / `color.dark` | table of 13 role -> `#rrggbb` | closed key set |
| `links` | `underline` (bool), `underline-thickness`, `underline-offset` | |
| `font-family`, `font-size`, `leading`, `space`, `layout` | open tables of string -> string | a theme may add its own keys |

**Colour values must be `#rrggbb`**: exactly six hex digits after `#`. Three-digit forms (`#fff`), CSS named colours (`orange`) and anything else are rejected rather than guessed: "expected a #rrggbb colour, found `…`".

**The colour palette is closed.** Adding `accent` to `[color.light]` fails: "unknown field `accent`, expected one of `text`, `text-muted`, `link`, `link-visited`, `focus`, `surface`, `surface-alt`, `surface-code`, `border`, `badge-fg`, `badge-bg`, `banner-fg`, `banner-bg`". Every role must exist in both schemes in the merged result. A missing one is a parse error, not a fallback to an unchecked colour.

**The five non-colour families are open.** A theme may add `[space] "9" = "6rem"`, `[layout] gutter = "2rem"` or `[font-family] display = "Georgia, serif"`, and each compiles to a custom property its stylesheet can read.

## Design tokens: the thirteen colour roles

Defaults from `assets/tokens.toml`. Each compiles to `--color-<role>`.

| Role | CSS property | Light default | Dark default | Where it is used (from the pair table) |
|---|---|---|---|---|
| `text` | `--color-text` | `#16181D` | `#E6E8EC` | body copy, headings, `dt`/`dd`, table cells |
| `text-muted` | `--color-text-muted` | `#565B63` | `#A7AEB8` | provenance line, footer, counts, nav section labels, snippet comments |
| `link` | `--color-link` | `#1F5AA8` | `#8AB4F0` | prose, index and nav links |
| `link-visited` | `--color-link-visited` | `#6E3D9B` | `#C9A6EC` | the same links once visited |
| `focus` | `--color-focus` | `#111418` | `#F2F4F8` | the focus ring |
| `surface` | `--color-surface` | `#FFFFFF` | `#14161A` | the page background |
| `surface-alt` | `--color-surface-alt` | `#F4F5F7` | `#1C1F24` | nav, table head, card |
| `surface-code` | `--color-surface-code` | `#EDEFF2` | `#22262C` | `code` and `pre`, IRIs and CURIEs |
| `border` | `--color-border` | `#787E88` | `#79808C` | table rules, `hr`, card and code edges |
| `badge-fg` | `--color-badge-fg` | `#22262C` | `#DCE0E6` | term-kind badge text |
| `badge-bg` | `--color-badge-bg` | `#E4E7EC` | `#2A2E35` | term-kind badge background |
| `banner-fg` | `--color-banner-fg` | `#16181D` | `#E6E8EC` | status banner heading |
| `banner-bg` | `--color-banner-bg` | `#E8EBF0` | `#23272E` | status banner background |

The only chromatic values in the default palette are `link` and `link-visited`, so a theme with its own brand colour overrides two lines per scheme and inherits everything else.

## Design tokens: the non-colour families

Five families plus the two link measurements.

| Family | Property name | Default keys and values |
|---|---|---|
| `[font-family]` | `--font-family-<key>` | `sans` = `system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, 'Noto Sans', sans-serif`; `mono` = `ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', 'Noto Sans Mono', monospace`; `ja` = `'Hiragino Sans', 'Noto Sans JP', 'Yu Gothic', Meiryo, system-ui, sans-serif` |
| `[font-size]` | `--font-size-<key>` | `-1` = `0.875rem`; `0` = `1rem`; `1` = `1.25rem`; `2` = `1.5rem`; `3` = `1.875rem`; `4` = `2.25rem` |
| `[leading]` | `--leading-<key>` | `body` = `1.6`; `list` = `1.5`; `heading` = `1.25` |
| `[space]` | `--space-<key>` | `1` = `0.25rem`; `2` = `0.5rem`; `3` = `0.75rem`; `4` = `1rem`; `5` = `1.5rem`; `6` = `2rem`; `7` = `3rem`; `8` = `4rem` |
| `[layout]` | `--<key>` (no prefix) | `measure` = `68ch`; `measure-wide` = `84ch`; `min-target` = `24px`; `breakpoint` = `60rem`; `radius` = `4px`; `border-width` = `1px`; `focus-width` = `3px`; `focus-offset` = `2px`; `scroll-padding` = `0px` |
| `[links]` | `--link-underline-thickness`, `--link-underline-offset` | `0.08em`, `0.15em` |

Notes on the naming rule:
- `[font-size]` key `-1` becomes `--font-size--1` (double hyphen). That is the property name the bundled stylesheet reads.
- `[layout]` keys take **no** family prefix: `min-target` becomes `--min-target`, not `--layout-min-target`.
- `[links] underline` is a boolean and is **not** emitted to CSS at all. It only selects which pairs the contrast gate checks.

What the defaults encode: `min-target` is WCAG 2.5.8 Target Size (Minimum), 24x24 CSS px, and `measure` bounds prose line length. `breakpoint` is where the two-column layout collapses, but it cannot reach a media query: CSS does not resolve `var()` inside `@media`, so the bundled stylesheet writes `60rem` literally and a theme that wants a different breakpoint must ship its own `@media` rule. 14pt = 18.5px and 18pt = 24px are the WCAG large-text thresholds, so in the default scale only `2` and above are large at normal weight and only `1` and above are large when bold.

Tokens the bundled `assets/theme.css` actually reads: all 13 colour roles, `--border-width`, `--focus-offset`, `--focus-width`, `--font-family-ja`, `--font-family-mono`, `--font-family-sans`, `--font-size--1` through `--font-size-4`, `--leading-body`, `--leading-heading`, `--leading-list`, `--link-underline-offset`, `--link-underline-thickness`, `--measure`, `--measure-wide`, `--min-target`, `--radius`, `--scroll-padding`, `--space-1` through `--space-8`.

## What `tokens.css` contains, and the colour-scheme options

`assets/tokens.css` is generated on every build from the merged `tokens.toml` and the chosen colour scheme. It is never taken from a theme (see the refusal above). Its first lines are `/* Generated from tokens.toml by iyo. Do not edit. */` and `/* theme: <meta.name> <meta.version> */`.

`--color-scheme <auto|light|dark>` on `iyo build` (env `IYO_COLOR_SCHEME`; `[site] color_scheme`; default `auto`) decides the structure:

| Value | What is emitted |
|---|---|
| `auto` | `:root { color-scheme: light; …light values… }`, then `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { color-scheme: dark; …dark values… } }`, then `:root[data-theme="light"] { … }` and `:root[data-theme="dark"] { … }` last so equal specificity resolves in their favour |
| `light` | a third comment line `/* published scheme: light only */`, then `:root { color-scheme: light; …light values… }` and nothing else. No media query, no `[data-theme]` blocks |
| `dark` | the same with dark: `/* published scheme: dark only */`, `:root { color-scheme: dark; …dark values… }` |

The non-colour families and the two link measurements are emitted once, in `:root`, in every mode. The `[data-theme]` and `prefers-color-scheme` blocks carry only the 13 colour roles.

Only the schemes a build actually publishes can stop it. Under `--color-scheme dark`, a failing light pair is reported as a note rather than an error: "N colour pairs fail in the light scheme, which this build does not publish (site.color_scheme). They would stop a build that did." The unpublished palette is still computed.

Full report: every build writes `a11y/contrast.json` into the output with keys `schema_version`, `formula`, `theme`, `underlined_links`, `pairs`, `findings` and `failed`. Each entry in `pairs` carries `scheme`, `foreground`, `background`, `fg_value`, `bg_value`, `ratio` (full precision), `ratio_2dp`, `required`, `basis`, `conditional`, `passes`, `site`. A default build produces 56 pairs.

## The contrast gate: pairs and thresholds

Runs on the merged tokens before a byte is written. Formula recorded in the report: "WCAG 2.2 relative luminance, (L1 + 0.05) / (L2 + 0.05)". The gate checks a fixed list of foreground/background combinations the default templates actually produce, in **both** schemes. It never takes the cross product of the 13 roles.

A colour the gate cannot parse stops the build whatever scheme it belongs to. That scheme is skipped entirely and contributes no pairs, so the refusal names the token (`theme.token-syntax`) instead of counting a failing pair.

Bases and thresholds:

| Basis | Threshold | Rule id | What it covers |
|---|---|---|---|
| `body-text` | 4.5:1 | `theme.contrast-text` | WCAG 1.4.3 Contrast (Minimum), text below 24px or below 18.5px bold |
| `large-text` | 3.0:1 | `theme.contrast-large-text` | the 1.4.3 large-text exception; no default pair uses it |
| `non-text` | 3.0:1 | `theme.contrast-non-text` | WCAG 1.4.11 Non-text Contrast: focus rings, borders, badge outlines |
| `link-vs-text` | 3.0:1 | `theme.contrast-link-vs-text` | WCAG 1.4.1 Use of Color, technique G183; checked only when links are not underlined |

**REQUIRED: 28 pairs, checked in every build, in both schemes (56 results).**

At 4.5:1 (`body-text`), 18 pairs:

| Foreground | Backgrounds | Quoted site |
|---|---|---|
| `text` | `surface`, `surface-alt`, `surface-code`, `banner-bg` | body copy/headings/dt/dd/table cells; nav, table head, card; code and pre, IRIs and CURIEs; status banner body |
| `text-muted` | `surface`, `surface-alt`, `surface-code`, `banner-bg` | provenance line, footer, counts; nav section labels; comments in example snippets; banner date line |
| `link` | `surface`, `surface-alt`, `surface-code`, `banner-bg` | prose and index links; contents nav links; linked term IRIs inside code; latest-version link in the banner |
| `link-visited` | `surface`, `surface-alt`, `surface-code`, `banner-bg` | the visited counterparts of the four above |
| `badge-fg` | `badge-bg` | term-kind badge text |
| `banner-fg` | `banner-bg` | banner heading |

At 3.0:1 (`non-text`), 10 pairs:

| Foreground | Backgrounds | Quoted site |
|---|---|---|
| `focus` | `surface`, `surface-alt`, `surface-code`, `badge-bg`, `banner-bg` | focus ring on the page; in the nav; on a link in code; on a badge link; on the banner link |
| `border` | `surface`, `surface-alt`, `surface-code`, `badge-bg`, `banner-bg` | table rules, hr, card and code edges; table rules on the header row; code-block edge from inside; badge outline; banner outline |

**CONDITIONAL: 2 pairs, checked only when `[links] underline = false`**, at 3.0:1 (`link-vs-text`): `link` against `text` ("link against surrounding prose") and `link-visited` against `text` ("visited link against surrounding prose").

The default `underline = true` is what makes the default palette legal: with underlines off the gate additionally requires link against text at 3:1, which no dark scheme with an off-white `text` satisfies. A theme setting `underline = false` and changing nothing else fails on all four conditional results (light link 2.61:1, light link-visited 2.37:1, dark link 1.73:1, dark link-visited 1.69:1).

Extending the table is a code change. There is no `[[contrast.extra]]` in `tokens.toml` (some source comments mention one; the parser rejects a `contrast` key). A theme that introduces a new surface cannot register the pairs it creates.

On failure the build prints, per failing pair: scheme, role names, measured ratio to two decimals, required ratio, the quoted site, and the two hex values, capped at 20 with "… and N more"; then "refusing to write: N failing colour pairs". Exit code 1.

The gate also fires on a schema mismatch (`theme.token-schema`) and on an unparseable colour (`theme.token-syntax`), each counted as a failure.

## `--theme-switch`: what it emits, and when it is refused

`--theme-switch` on `iyo build` (`[site] theme_switch`, default `false`) sets `site.theme_switch`, which the default `base.html.jinja` branches on. It emits two inline scripts and one button, and nothing else on the page needs JavaScript.

1. **Head script** (`partials/theme-switch.html.jinja`), included immediately after `<meta name="viewport">`, before the stylesheets. It reads `localStorage["iyo-theme"]` and, if the value is `"light"` or `"dark"`, sets `data-theme` on the root element. It is inline and first so the stored choice applies before first paint. A `try`/`catch` covers private mode and disabled storage, in which case the operating-system preference still applies.
2. **Button** (`partials/theme-switch-button.html.jinja`), at the end of the site header: `<button type="button" class="theme-switch" id="iyo-theme-switch" hidden>` containing `<span class="theme-switch-label">Theme</span>` and `<span class="theme-switch-value" id="iyo-theme-value">system</span>`.
3. **Button script**, immediately after it. Cycles `system -> light -> dark`. `system` removes the `data-theme` attribute and the storage key. The other two set both. It updates the value span and sets `aria-label` to `"Colour scheme: <mode>. Activate to change."`, then sets `button.hidden = false`.

The button ships `hidden` and the script unhides it, so a reader without JavaScript sees no control at all rather than a dead one, and `hidden` (not CSS) removes it from the accessibility tree too.

`data-theme` is read by the generated `tokens.css`.

**Refusal:** `--theme-switch` together with `--color-scheme light` or `dark` is rejected, exit 2 (`USAGE`): "--theme-switch offers a choice between schemes, and --color-scheme <x> publishes only one", hint "drop one of the two: --color-scheme auto to offer both, or no --theme-switch to publish the one". Forcing one scheme emits no `[data-theme]` blocks.

## The PDF template: spec.typ and model.json

`iyo build --pdf` (`[site] pdf`) writes two files per namespace into `<mount>pdf/`: `model.json` (the data) and `spec.typ` (the Typst source). `iyo pdf <dir>` then runs Typst over them. The two steps are separate because Typst is an external binary the build cannot assume is installed.

**The host language generates no Typst markup.** `spec.typ` is written out unchanged: the bundled copy, or the theme's `pdf/spec.typ` if it has one. It is **not** run through minijinja, so a file containing `{{ not_templated }} {% raw %}` reaches the output character for character. Theming the PDF means editing that one file.

**`spec.typ` reads `model.json` itself**, with `#let model = json("model.json")` on its first active line. The tool passes it no data. Typst is invoked with `--root <the pdf dir>`, so the relative `json("model.json")` resolves beside it.

**`model.json` top level, exactly three keys:**

| Key | Contents |
|---|---|
| `site` | exactly three fields: `title`, `base_url`, `lang` |
| `document` | the full serialised `DocumentView` for this namespace — the same object `document.html.jinja` receives as `document`, with the same field names and the same absent-when-`None` behaviour |
| `pdf` | one field, `stem`: the basename the compiled file must take |

The `document` keys a build produces: `abstract_paragraphs`, `breaking_count`, `change_count`, `comment`, `contributors`, `created`, `creators`, `description`, `has_part`, `hierarchy`, `iri`, `kind`, `license`, `llms_url`, `namespace`, `pdf_url`, `prefix`, `profile`, `publishers`, `removed`, `reused`, `sections`, `see_also`, `siblings`, `status`, `status_iri`, `term_count`, `title`, `url`, `version`, `version_iri`, `versions`, `versions_url`.

**What the bundled `spec.typ` does**, as a starting point to edit: sets `document(title:, author:, keywords:, description:)` from the model; `text(lang:)` from `site.lang`; A4, 2.2cm/2.4cm margins, page numbering, a footer carrying the title; three heading levels; a `facts` grid used for both metadata and per-term facts; a title page; a `#outline`; then one section per `document.sections` entry and one level-2 heading per term, each with a `#label(term.iri)` so cross-references inside the PDF jump within the document and references to anything else become web links.

Two warnings the bundled file carries for anyone editing it: keep `document(...)` populated, because PDF/A requires a title and `text(lang:)` is what carries the language into the tagged structure; and do not name a variable `label`, because `label()` is the built-in that turns an IRI into an anchor.

**Compilation** (`iyo pdf`): Typst is run with `--ignore-system-fonts` (embedded fonts only, so two machines produce the same file), `--pdf-standard`, and `--creation-timestamp` taken from the vocabulary's own date metadata rather than the clock (epoch 0 if the RDF records none). `--font-path DIR` adds font directories. `--pdf-standard LIST` overrides the default, which is chosen from the installed Typst: `ua-1,a-2a` from 0.15 onward, `a-2a` before it.

## Build-stopping checks a theme override can trip

Beyond the contrast gate, three things stop `iyo build`.

1. **Undefined custom property.** Any `var(--name)` in an output stylesheet with no definition and no fallback (see the assets section). Build aborts.
2. **`assets/tokens.css` in the theme.** Refused outright (see the assets section).
3. **The HTML auditor.** Every page the build produced is parsed and checked. An error-level finding refuses the write with "refusing to write: N accessibility errors in the rendered pages", exit 1. These are the rules a custom template can break:

| Rule | Level |
|---|---|
| `a11y.no-html-element`, `a11y.no-lang`, `a11y.lang-invalid`, `a11y.lang-part-invalid` | error |
| `a11y.no-title`, `a11y.no-viewport`, `a11y.charset-late` | error |
| `a11y.missing-landmark`, `a11y.multiple-main`, `a11y.unnamed-nav` | error |
| `a11y.no-h1`, `a11y.heading-skip` | error |
| `a11y.multiple-h1` | warning |
| `a11y.empty-id`, `a11y.invalid-id`, `a11y.duplicate-id`, `a11y.dangling-fragment` | error |
| `a11y.empty-link` | error |
| `a11y.table-no-headers` | error |
| `a11y.table-no-caption`, `a11y.th-no-scope` | warning |
| `a11y.broken-internal-link` | warning |
| `html.absolute-nav-link` | error |
| `html.escaped-markup` | error |
| `markdown.absolute-link` | error |
| `markdown.broken-link` | warning |

`html.absolute-nav-link` is what enforces the `| rel` filter on navigation hrefs. `html.escaped-markup` catches the `~`-and-`| safe` mistake described in the filters section. An anchor whose label variable was never supplied trips `a11y.empty-link`.

The full audit is written to `a11y/structure.json` in the output (`schema_version`, `generator`, `pages_checked`, `errors`, `warnings`, `issues`). Warnings do not stop the build unless `--strict` is passed.

The theme layer is designed to meet WCAG 2.2 Level AA. What is mechanically checked is (a) the 28 colour pairs above in each published scheme against the thresholds in the table, and (b) the structural rules listed here on every generated page. Nothing beyond those two lists is checked by the tool.

## Commands and options that touch theming

Complete, from `--help` output of the release binary.

`iyo build` options relevant to a theme:

| Option | Env | Default | Effect |
|---|---|---|---|
| `--theme <DIR>` | `IYO_THEME` | none (built-in theme) | the overlay directory |
| `--color-scheme <auto\|light\|dark>` | `IYO_COLOR_SCHEME` | `auto` | which schemes `tokens.css` publishes, and which schemes the gate can fail on |
| `--theme-switch` | — | off | emit the light/dark control; conflicts with a forced `--color-scheme` |
| `--pdf` | — | off | write `pdf/spec.typ` and `pdf/model.json` per namespace |
| `--base-path <PATH>` | `IYO_BASE_PATH` | path of `--base-url` | only the 404 page's root-relative links use it |
| `--strict` | — | off | refuse to write on warnings as well as errors |
| `--dry-run`, `-n` | — | off | report what would be written, including the digest, writing nothing |

`iyo pdf <dir>` options: `--pdf-standard <LIST>`, `--typst-bin <PATH>` (env `IYO_TYPST_BIN`, default `typst`), `--font-path <DIR>`, `--dry-run`.

`iyo model <input>...` prints the intermediate model as JSON and is described in its own help as "the contract that renderers, themes and any future plugin read". It prints the *model*, not the view model that templates receive. The two have different field names.

`iyo.toml` keys under `[site]` that a theme's behaviour depends on: `theme_switch` (bool, default `false`), `color_scheme` (string, default `"auto"`), `base_path`, `doc_license`, `title`, `lang`, `cite_as`, `pdf`, `link_style`. There is no `theme` key. The theme directory is a command-line or environment input only.

Exit codes: 1 `FINDINGS` (contrast failures, audit errors), 2 `USAGE` (the `--theme-switch` conflict), 3 `INPUT` (bad theme directory, bad `tokens.toml`, unknown config key).
