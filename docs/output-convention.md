# Output convention

[README](../README.md) · [cli](cli.md) · [rules](rules.md) · [theming](theming.md) · **output-convention** · [ci](ci.md)

The file layout, URL patterns, content negotiation and manifest that a build
produces. Where this implementation makes a choice the convention does not
require, the page says which is which.

> **Licence.** This page is available under
> [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/), separately from the
> Apache-2.0 licence covering the rest of the repository, so that another
> implementation can follow the convention without taking on the tool's licence.
> Attribute it as "iyo output convention". This grant covers this page only.

## URL patterns

Four URL families, all derived from two declared strings: the **identity IRI base** (`iri_base`, what the RDF mints) and the **document base** (`doc_base` = `--base-url` + mount).

| Thing | Pattern (`flat`) | Pattern (`dir`) | Example from the fixture |
| --- | --- | --- | --- |
| Term page | `<doc_base><Local>` | `<doc_base><Local>/` | `https://example.org/vocab/Widget` |
| Term sibling | `<doc_base><Local><ext>` | `<doc_base><Local>/index<ext>` | `https://example.org/vocab/Widget.ttl`, `…/vocabulary/category/index/index.ttl` |
| Namespace document | `<doc_base>` | same | `https://example.org/vocab/` |
| Namespace sibling | `<doc_base>index.md`, `<doc_base><stem>.ttl`, `<doc_base><stem>.jsonld` | same | `…/vocab/index.md`, `…/vocab/ex.ttl` |
| Release snapshot | `<doc_base><version>/…` | same | `https://example.org/vocab/0.1.0/Widget` |

**Mount.** Default is the path part of the namespace IRI, kept even when the documents are served from another host. `https://example.org/vocab/` → `/vocab/`; `https://example.org/vocabulary/category/` → `/vocabulary/category/`; a site-root namespace → `""`; a hash namespace `http://ex.org/vocab#` → `vocab/`. Overridable per namespace with `namespaces."<iri>".mount` (a value without a trailing slash is normalised to one).

**Identity and location are separate.** `iyo build testdata/mini --base-url https://docs.example.net/v/` gives `site_root https://docs.example.net/v/`, `doc_base https://docs.example.net/v/vocab/` and `iri_base https://example.org/vocab/`. The IRI base is never rewritten.

**`--base-url` default:** the origin of the release's root document. The fixture declares no base URL and the manifest still says `"site_root": "https://example.org/"`.

**Layout.** `flat` is the default for every namespace. `dir` is opt-in per namespace via `namespaces."<iri>".url_style = "dir"`. There is no CLI flag for it.

**Per-term fallback to `dir` inside a `flat` namespace** happens for exactly two reasons, and the affected local names are listed in `dir_terms`:
1. The local name is a file stem the site owns: `index` in any namespace, plus `404` when the mount is the site root.
2. The local name differs from another local name in the same namespace only by ASCII case. Within a colliding group the names are sorted and the **first keeps `flat`**, the rest go to `dir`. Fixture: `Format` → `vocab/Format.html`, `format` → `vocab/format/index.html`, `dir_terms: ["format", "index"]`.

**Navigation vs identity.** `--link-style iri` (default) points navigation hrefs at the term IRI; `--link-style file` points them at the file (`../vocab/Thing.html` instead of `../vocab/Thing`). `rel="canonical"` and `manifest.json` are byte-identical under both.

## File layout per term

Four files per local term, always, in every layout. There is no configuration that adds or removes a representation.

| Representation | `flat` file | `dir` file | Manifest `media_type` | Served `Content-Type` |
| --- | --- | --- | --- | --- |
| HTML | `<mount><Local>.html` | `<mount><Local>/index.html` | `text/html` | `text/html; charset=utf-8` |
| Markdown | `<mount><Local>.md` | `<mount><Local>/index.md` | `text/markdown` | `text/markdown; charset=utf-8` |
| Turtle | `<mount><Local>.ttl` | `<mount><Local>/index.ttl` | `text/turtle` | `text/turtle; charset=utf-8` |
| JSON-LD | `<mount><Local>.jsonld` | `<mount><Local>/index.jsonld` | `application/ld+json` | `application/ld+json` (no charset) |

Case is preserved verbatim in filenames and URLs.

**Foreign (reused) terms get no per-term files.** `dcterms:title` is declared in the fixture and produces no file. It appears only as a row in the namespace document's reused-terms table, with `id="dcterms_title"`, the prefix and local name joined by an underscore.

**A term whose identity is in namespace A but whose definition is in document B keeps A's files.** `ex:WidgetShape` is defined in `shapes.ttl` (document `https://example.org/vocab/shapes/`) but its IRI is `https://example.org/vocab/WidgetShape`, so its four files are at `vocab/WidgetShape.*` while it is *listed* in `vocab/shapes/index.html` and `vocab/shapes/llms.txt`.

**Turtle sibling** = the concise bounded description of the term with the release's prefix declarations plus `rdfs:isDefinedBy`. Byte-identical to the fenced block under `## Statements` in the Markdown sibling.

**JSON-LD sibling** is self-contained: an inlined `@context` subset, `@id` as an absolute IRI, other IRIs as CURIEs, blank nodes nested, `xsd:boolean`/`xsd:integer` as native JSON (`"sh:closed": false`, `"sh:minCount": 1`).

**Per-term Markdown structure** (from `vocab/Widget.md`, `vocab/category.md`):
1. Blockquote: `> Part of <title>. Index: <llms.txt URL>. Canonical page: <term HTML URL>`
2. `# <label> (<curie>)`
3. Fact bullets: `IRI` (backticked), `Kind`, `Defined by` (with version and status), `Label`, then kind-specific facts (`Subclass of`, `Domain`, `Range`, `Field name`, `Cardinality`, `Values from`), each SHACL-derived fact suffixed `(source: SHACL <shape>)`
4. `## Definition` + the definition + an italic `*Source: rdfs:comment*` line
5. `## Notes` / `## Examples` when the term carries `skos:note` / example annotations
6. `## Record template from <shape>` (class page) or the constraint equivalent: a table of Field / Property / Values / Count / Description
7. `## Statements`: a `turtle` fence, identical to the `.ttl` sibling
8. `## Also available as`: HTML, Markdown, Turtle, JSON-LD

**`--md-frontmatter`** prefixes YAML before the blockquote: `none` (default, no front matter), `hugo`, `mkdocs`, `jekyll`. All three emit `title`, `iri`, `kind`, `weight` (1-based position among that namespace's own terms). `hugo` additionally emits `url:` (the case-exact request path, e.g. `/vocab/Widget`) and renames the namespace's Markdown document from `index.md` to `_index.md`.

## File layout per namespace

Written at `<mount>` for every namespace:

| File | Content | Always? |
| --- | --- | --- |
| `index.html` | Namespace document | yes |
| `index.md` (`_index.md` under `--md-frontmatter hugo`) | Markdown sibling of the namespace document | yes |
| `<stem>.ttl` | The namespace's graph, from the file its document came from | yes |
| `<stem>.jsonld` | Same graph as JSON-LD | yes |
| `context.jsonld` | JSON-LD context for this namespace | yes |
| `llms.txt` | Per-namespace agent index | yes |
| `terms.json` | This namespace's term index | yes |
| `versions.ttl` | `dcterms:hasVersion` / `dcterms:isVersionOf` / `owl:versionInfo` triples | only when ≥1 snapshot was written for this namespace |
| `changes.md` | Diff against the previous release, scoped to this namespace | only with `--previous`, and only when that namespace has changes |
| `<version>/` | Full snapshot directory | see Versioned snapshots |

**`<stem>` default** = the namespace's preferred prefix (`vann:preferredNamespacePrefix`); if absent, the last path segment of the mount; if that is empty, `vocabulary`. Overridable with `namespaces."<iri>".stem`. Fixture: `vocab/ex.ttl`, `vocabulary/category/ex-cat.ttl`, `vocab/shapes/shapes.ttl` (that document declares no prefix, so the stem falls to the mount segment).

**Namespace document sections** (`vocab/index.html`): status banner (`Under development, version 0.1.0`) → `<h1>` → the IRI in `<code lang="zxx" translate="no">` → `Metadata` → `Hierarchy` → one `<h2>` per term kind present (`Classes`, `Object properties`, `Datatype properties`, …), each local term carrying `id="<Local>"` → `Terms reused from other vocabularies` (a table; foreign ids are `<prefix>_<local>`) → `Namespaces` → `Other formats` → footer.

**`terms.json`** is a JSON array sorted by IRI. Exactly ten keys per entry, no more:

```json
{"iri": "…", "curie": "ex:Thing", "kind": "class", "label": "Thing",
 "definition": "Anything at all.", "deprecated": false,
 "namespace": "https://example.org/vocab/",
 "html": "…/Thing", "md": "…/Thing.md", "ttl": "…/Thing.ttl"}
```

`curie` and `definition` are omitted when absent. The root `terms.json` is the union across namespaces, same shape.

**`llms.txt` per namespace**, in this fixed order:
1. `# <title>`
2. Blockquote: description, namespace IRI, prefix, version, status, licence
3. A "how to use" paragraph naming the `@prefix` declaration to make
4. One `## <section>` per term kind present, in a fixed kind order: `Classes`, `Object properties`, `Datatype properties`, `Annotation properties`, `Properties`, `Datatypes`, `Encoding schemes`, `Concepts`, `Collections`, `Node shapes`, `Property shapes`, `Individuals`, `Other terms`. Each line is `- [label](url.md): definition`; deprecated terms are prefixed `(deprecated, replaced by X)`.
5. `## Terms reused from other vocabularies`: `` - `curie`: note `` (no link; foreign terms have no page)
6. `## Serialisations`: Turtle, Release Turtle, JSON-LD, JSON-LD context, Term index, then `Version history` and one line per release when snapshots exist
7. `## Optional`: Changes (with `--previous`), PDF (with `--pdf`), Human documentation, Markdown of this document, `llms-full.txt`, Companion dataset (when `llms.data_site` is set)

**Size rule:** above `llms.max_terms` (default `500`) the per-kind term lists are replaced by a single `## Terms` section linking that namespace's `terms.json`.

## Site-level files

Written once at the site root:

| File | Content |
| --- | --- |
| `manifest.json` | The negotiation manifest (see below) |
| `release.ttl` | Every input document unioned |
| `release.jsonld` | The same graph as JSON-LD |
| `context.jsonld` | The whole release's JSON-LD context |
| `terms.json` | Union of the per-namespace term indexes |
| `llms.txt` | Root index: `## Vocabularies` (one line per namespace's `llms.txt`), `## Serialisations`, `## Optional` |
| `llms-full.txt` | Every term page's Markdown concatenated |
| `versions.json` | Snapshot record (see below) |
| `index.html` | Site landing page: `Vocabularies`, `For machines` |
| `404.html` | Not-found page; **the only page with root-relative links** |
| `assets/tokens.css`, `assets/theme.css` | The two stylesheets of the bundled theme |
| `a11y/contrast.json` | Keys: `schema_version, formula, theme, underlined_links, pairs, findings, failed` |
| `a11y/structure.json` | Keys: `schema_version, generator, pages_checked, errors, warnings, issues` |
| `adapters/<host>/…` | Only with `--host` |

Every page except `404.html` links relatively, so a built tree browses from any subdirectory or from `file://`. `404.html` is served by the host at whatever path was requested, so its links are root-relative. `--base-path /repo/` (or `site.base_path`) sets that root and changes **only** `404.html`.

`versions.json` top-level shape:

```json
{"schema_version": "0.1", "generator": "iyo 0.1.0", "policy": "version-iri",
 "latest": "release",
 "cache_control": {"snapshot": "public, max-age=31536000, immutable",
                   "latest": "public, max-age=300, must-revalidate"},
 "namespaces": [{"iri": "…", "latest_url": "…", "versions": [ …Snapshot… ]}],
 "not_written": ["<…> declares owl:versionIRI <…>, which is not …, so no snapshot was written there; pass --snapshots all to write one anyway"]}
```

## Versioned snapshots

A snapshot is the same namespace re-rendered with the version segment appended to its mount, so it carries the complete per-namespace and per-term set (`vocab/0.1.0/` in the fixture holds 19 files: all four siblings for each of the six terms, plus `index.html`, `index.md`, `ex.ttl`, `ex.jsonld`, `context.jsonld`, `llms.txt`, `terms.json`).

**Where the version string comes from**, first match wins: `owl:versionInfo` → `dcterms:issued` → `dcterms:modified` → `--release` / `site.release`. The source is recorded as `"source": "version-info" | "issued" | "modified" | "configured"`.

**What is accepted as a segment.** Not configurable. At most 64 characters, only `[A-Za-z0-9._+-]`, not starting with `.` or `-`; after splitting off the first `-`/`+` suffix, the core must be either all-digit dot-separated parts (`1`, `1.2`, `0.1.0`) or all-digit hyphen-separated parts containing a hyphen (`2026-04-28`). Accepted: `1`, `1.2`, `0.1.0`, `0.1.0-draft`, `2026-04-28`, `2026-04-28-2`. Rejected: `draft`, `under development`, `../escape`, empty.

**`--snapshots` policy** (default `version-iri`):

| Value | Behaviour |
| --- | --- |
| `version-iri` | Write a snapshot only where `owl:versionIRI` equals `<doc_base><segment>/`. A mismatch is reported in `versions.json.not_written` and as a build note. A namespace with no version IRI is silently skipped. |
| `all` | Write for every namespace carrying any usable version string. Fixture: adds `/vocab/shapes/0.1.0/`, which has `owl:versionInfo` but no `owl:versionIRI`. |
| `none` | Write none. |

**A version segment that equals one of that namespace's own local names** causes the snapshot to be skipped (never renamed), with a line in `not_written`.

**`versions.ttl`** (latest only) carries `<B> dcterms:hasVersion <B/V/>` and `<B/V/> dcterms:isVersionOf <B> ; owl:versionInfo "V"`.

**Snapshot files are not byte-identical to latest.** Turtle and JSON-LD siblings are, as are `ex.ttl`, `ex.jsonld` and `context.jsonld`. HTML, Markdown, `terms.json` and `llms.txt` differ, because every URL they name is rewritten into the snapshot: `canonical` becomes `…/vocab/0.1.0/Widget`, the Markdown blockquote points at `…/vocab/0.1.0/llms.txt`, `terms.json` URLs carry the segment.

## Content negotiation — the rules

Negotiation applies to **term paths only**. A namespace document, a nested namespace, a reserved segment and a sibling asked for by name are all left to the file layer.

Selection algorithm, in order:

1. **Query override.** If the query string carries `format=`, `_mediatype=` or `_profile=` (first of those keys found wins), the value is run through the alias table. If it names a representation the namespace publishes, that one is used and `Accept` is ignored.
2. **`Accept` parsed per RFC 9110**: split on `,`, take the media type before the first `;`, lowercase it, read `q=` from the parameters (default `1`), drop entries with `q <= 0`, sort by q descending and **stably by the client's own order within a q**.
3. **Explicit types first**, walked in that sorted order. The first one the namespace publishes wins. Wildcards are skipped in this pass, so an explicit type beats a wildcard whatever the q-values say.
4. **Wildcard.** If any `*/*` or `type/*` survives: `text/*` prefers a `text/…` representation that is also the default, else any `text/…`. Every other wildcard gives `default_type`.
5. **RDF fallback.** If nothing matched and the client named any type whose string contains `rdf`, `turtle`, `ld+json`, `n-triples`, `n3` or `trig`, the **first declared** representation matching that same test is used.
6. **Otherwise `default_type`** (`text/html`). **Nothing ever answers 406.**

Then: if the chosen type is `default_type`, answer **200** with the file and `Vary: Accept`; otherwise **303** (`status_code`) with `Location` = the sibling URL.

**A request with no `Accept` header** parses to an empty list, so steps 3–5 all miss and step 6 gives HTML with 200.

Responses for `/vocab/Widget` (`iyo serve`):

| `Accept` / query | Result |
| --- | --- |
| *(no header)* | `200 text/html; charset=utf-8` |
| `text/html` | `200` |
| `text/turtle` | `303 → /vocab/Widget.ttl` |
| `text/markdown` | `303 → /vocab/Widget.md` |
| `application/ld+json` | `303 → /vocab/Widget.jsonld` |
| `*/*` | `200` HTML |
| `text/*` | `200` HTML |
| `application/*` | `200` HTML |
| `image/png` | `200` HTML |
| `text/turtle;q=0.1, text/html` | `200` HTML |
| `text/html;q=0.1, text/turtle` | `303 → .ttl` |
| `text/html;q=0, text/turtle` | `303 → .ttl` |
| `*/*;q=1.0, text/turtle;q=0.1` | `303 → .ttl` |
| `application/rdf+xml` | `303 → /vocab/Widget.ttl` (RDF fallback) |
| `application/xml` | `200` HTML (not RDF by the substring test) |
| `text/turtle, text/markdown` | `303 → .ttl` |
| `text/markdown, text/turtle` | `303 → .md` |
| `?format=ttl` | `303 → .ttl` |
| `?format=json` | `303 → .jsonld` |
| `?format=md` with `Accept: text/turtle` | `303 → .md` (override wins) |
| `?_mediatype=text/markdown` | `303 → .md` |
| `?_profile=ttl` | `303 → .ttl` |
| `?format=rdf`, `?format=nt` | `200` HTML — no such representation is published, so the override is ignored |

**Alias table** (`iyo serve`): `html`→`text/html`; `md`,`markdown`→`text/markdown`; `ttl`,`turtle`→`text/turtle`; `jsonld`,`json`→`application/ld+json`; `rdf`,`xml`→`application/rdf+xml`; `nt`→`application/n-triples`; any value containing `/` is passed through as a media type. The generated Cloudflare Worker implements a narrower table (`html md ttl jsonld rdf nt json` plus slash-passthrough), so `markdown`, `turtle` and `xml` work on `iyo serve` but not on that Worker.

**Perimeter rules.** Each of these is a `PassThrough` to the file layer, which answers 404:
- a local name not in the namespace's `terms` list (`resolver_type: "strict"`);
- a case variant of a real term;
- a path still containing `/` after the mount (`/vocab/Thing/extra`);
- a path ending in a suffix the namespace publishes (`/vocab/Widget.ttl` is a file request, not a negotiation);
- a path ending in a suffix nobody publishes (`/vocab/Thing.nt`);
- a reserved segment (`/vocab/shapes`), a namespace with no terms, and a segment that is not a release.

**Namespace matching** is longest-mount-wins, so `/vocabulary/category/round` resolves against the scheme and not its parent. A path whose first segment after the mount matches a declared release segment is resolved against a synthesised snapshot namespace with the same terms, layout and representations.

**Headers on every negotiated response** (`iyo serve`): `Access-Control-Allow-Origin: *`, `Vary: Accept`, `Link: …`, `Cache-Control`. Cache-Control is `public, max-age=300, must-revalidate` for latest and `public, max-age=31536000, immutable` for anything inside a release.

## Link relations

Two carriers, with different contents.

**HTTP `Link` header**, emitted by `iyo serve` and by the generated Cloudflare Worker on any negotiated response (200 or 303):

| Relation | Form | Notes |
| --- | --- | --- |
| `canonical` | absolute | The term's HTML URL at the manifest's `site_root` |
| `cite-as` | absolute | `iri_base + local` — the identity IRI, never the document URL |
| `alternate` | relative reference, with `type="…"` | One per representation **other than the one being served**, so a 303 to Turtle lists HTML, Markdown and JSON-LD |
| `describedby` | relative reference, `type="text/plain"` | The covering `llms.txt` |

For `GET /vocab/Widget` with no `Accept`:

```
Link: <https://example.org/vocab/Widget>; rel="canonical",
      <https://example.org/vocab/Widget>; rel="cite-as",
      <Widget.md>; rel="alternate"; type="text/markdown",
      <Widget.ttl>; rel="alternate"; type="text/turtle",
      <Widget.jsonld>; rel="alternate"; type="application/ld+json",
      <llms.txt>; rel="describedby"; type="text/plain"
```

`canonical` and `cite-as` are absolute. `alternate` and `describedby` are relative references, so the same header is correct from a preview or staging host. A term served from a directory reaches its siblings as `index.ttl` and its namespace's files as `../llms.txt`.

**HTML `<link>` elements** on a term page, the floor on hosts that cannot set headers:

| Relation | Emitted? | Value |
| --- | --- | --- |
| `canonical` | always | absolute term HTML URL |
| `alternate` ×3 | always | one per other representation, with `type` and a `title` (`Markdown`, `Turtle`, `JSON-LD`) |
| `describedby` | always | relative path to the covering `llms.txt` |
| `collection` | when known | relative path to the namespace document, `type="text/html"` |
| `license` | when the RDF declares `dcterms:license` | absolute licence URL |
| `stylesheet` ×2 | always | `assets/tokens.css`, `assets/theme.css` |
| `cite-as` | **only when `site.cite_as = true`** | absolute identity IRI |

`site.cite_as` defaults to `false` and has no CLI flag. Signposting makes `rel="cite-as"` wrong when the identity IRI does not resolve back to the page, which is the case on a host that cannot negotiate. With `cite_as = true` the element appears on term pages *and* on namespace documents.

Also in the HTML `<head>` of every term page: `<title>`, `<meta name="description">`, `<meta name="generator" content="iyo 0.1.0">`, and a `schema.org` JSON-LD `@graph` of a `WebPage` plus a `DefinedTerm` (`@id`, `name`, `termCode`, `url`, `description`, `inDefinedTermSet`).

The term heading carries `id="<Local>"`. Section headings are namespaced `id="iyo-…"` so they cannot collide with a term name.

## manifest.json — top-level shape and field meanings

Written at the site root on every build. There is no flag to suppress it.

```json
{"convention": "iyo/1",
 "generator": {"name": "iyo", "version": "0.1.0"},
 "site_root": "https://example.org/",
 "namespaces": [ … ]}
```

| Top-level field | Meaning |
| --- | --- |
| `convention` | Literal `"iyo/1"` — the version of this output convention |
| `generator` | Object with `name` and `version` |
| `site_root` | `--base-url`, always with a trailing slash |
| `namespaces` | One entry per namespace, in the release's namespace order |

Namespace entry. Fields marked *optional* are omitted entirely when absent.

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | The preferred prefix; if the namespace declares none, the mount with `/` replaced by `-` (fixture: `vocab-shapes`) |
| `kind` | string | `ontology`, `scheme`, `shapes`, or `document` when there is no document |
| `iri_base` | string | What the RDF mints. Byte-exact, never rewritten |
| `doc_base` | string | `site_root` + `mount`. Where the documents are served |
| `mount` | string | Site path with leading **and** trailing slash (`/vocab/`) |
| `resolver_prefix` | string, *optional* | Path under a redirect host standing for this namespace. Only from `namespaces."<iri>".resolver_prefix` |
| `layout` | `"flat"` \| `"dir"` | The namespace default |
| `dir_terms` | string[] | Local names that use the directory layout against that default. Sorted. A host that reads `layout` alone routes exactly these to the wrong file |
| `resolver_type` | string | Always `"strict"` in this version: a local name not in `terms` answers 404 |
| `prefix` | string, *optional* | `vann:preferredNamespacePrefix` |
| `title` | string, *optional* | `dcterms:title` of the namespace document |
| `version` | string, *optional* | `owl:versionInfo` |
| `version_iri` | string, *optional* | `owl:versionIRI` |
| `status` | string, *optional* | `adms:status` IRI |
| `licence` | string, *optional* | `dcterms:license` IRI |
| `default_type` | string | Always `"text/html"` in this version. What a wildcard and an unmatched `Accept` get, and the type served with 200 instead of redirected |
| `status_code` | number | Always `303`. The redirect status for a non-default representation |
| `cache_max_age` | number | Always `86400`. Advisory; distinct from `cache_control` |
| `representations` | array | **Ordered.** The order is the tie-break for the RDF fallback and for choosing a default when `default_type` is not published |
| `terms` | string[] | Every local name this namespace publishes, sorted by byte order (so `Thing, Widget, WidgetShape, category, pairedWith, serial`) |
| `reserved` | string[] | Path segments under this mount that are documents or releases, not terms. Sorted, deduplicated. Fixture: `["0.1.0", "shapes"]` |
| `llms_txt` | string | Absolute URL of this namespace's agent index |
| `versions` | array | One Snapshot object per release actually written |
| `cache_control` | string | `public, max-age=300, must-revalidate` |
| `snapshot_cache_control` | string | `public, max-age=31536000, immutable` |

`representations[]` entry: `media_type` (bare type, no charset), `suffix` (`""` for HTML, `".md"`, `".ttl"`, `".jsonld"`; `null` would mean namespace-level only and is never emitted by this version), `namespace_file` (path of that representation of the **namespace document**, relative to the site root, e.g. `"vocab/ex.ttl"`).

`versions[]` entry: `namespace`, `segment`, `source` (`version-info` \| `issued` \| `modified` \| `configured`), `url`, `version_iri` (*optional*), `version_iri_resolves` (boolean, true when `owl:versionIRI` equals `url`).

The manifest is unaffected by `--link-style`, by `--theme`, and by `--md-frontmatter` except through `namespace_file` for Markdown (`_index.md` under `hugo`).

## Host adapters compiled from the manifest

`--host <HOST>` (repeatable; also `site.hosts`) writes `adapters/<host>/`. Accepted values: `cloudflare`, `apache`, `vercel`, `dcmi-ns`, `github-pages`. Every adapter is a function of `manifest.json` alone. Each one also writes a `README.md` stating what that host cannot do.

| Host | Files | Runs code | Stated limits |
| --- | --- | --- | --- |
| `cloudflare` | `_headers`, `_redirects`, `worker.js`, `wrangler.toml`, `README.md` | yes | none |
| `apache` | `.htaccess`, `README.md` | no | `Accept` matched by regex, no q-value ranking; no per-term `Link` headers |
| `vercel` | `vercel.json`, `README.md` | no | no q-value ranking, order of rules decides; `Link` per namespace, not per term |
| `dcmi-ns` | `resolver/<id>.json` per namespace, `README.md` | no | one representation per term (no suffix); no versioned paths, no anchor map |
| `github-pages` | `.nojekyll`, `RESOLUTION.md`, `README.md` | no | no negotiation at all; extensionless IRIs do not resolve |

Details that differ between hosts:

- **Trailing-slash redirect status** for a release root: `_redirects` uses `301`, `.htaccess` uses `R=302`, `vercel.json` uses `308`.
- **`?format=` support**: `.htaccess` matches `format=md|ttl|jsonld` in `QUERY_STRING`; `vercel.json` emits **no** query rules (header rules only); `worker.js` handles `format`, `_mediatype`, `_profile`.
- **Cloudflare `_headers`**: `/*` gets `Access-Control-Allow-Origin: *`, `X-Content-Type-Options: nosniff`, and a fixed `Content-Signal: search=yes, ai-train=yes`; `/*.md`, `/*.ttl`, `/*.jsonld` get their `Content-Type`, `Vary: Accept` and CORS; each mount gets its `Cache-Control`, with the release mount listed first so it wins.
- **`wrangler.toml`** sets `not_found_handling = "404-page"` and scopes `run_worker_first` to the mounts with `!`-exclusions for `*.md`, `*.ttl`, `*.jsonld`, so a request for a file that exists never wakes the Worker. `compatibility_date` is pinned (`2024-11-01`) so two builds of one release produce the same file.
- **`dcmi-ns` resolver entry** keys: `id, namespace, documentBase, prefix, defaultMediaType, statusCode, maxAge, resolverType, representations[{mediaType, append, file}], terms, dirTerms, reserved`. When `dir_terms` is non-empty the README names the exact IRIs that will be mis-routed, because the schema cannot express a per-term layout.
- **`--host github-pages`** writes a `RESOLUTION.md` naming, per namespace, which URLs will and will not resolve.

## Configuration that changes layout, URLs or negotiation

`iyo.toml` (or `--config <FILE>`; picked up automatically as `./iyo.toml`). Unknown keys are **refused**, not ignored, with a message listing the accepted keys.

`[site]`, with defaults as compiled in:

| Key | Default | Effect |
| --- | --- | --- |
| `base_url` | `"/"` | Site root; a trailing slash is added if missing. `--base-url` overrides. Defaults to the origin of the release's root document when neither is given |
| `lang` | `"en"` | `<html lang>` and language selection for labels |
| `doc_license` | none | Adds the second licence to the footer (`Vocabulary under X; this documentation under Y`). Without it the footer names one licence |
| `title` | none | Site title |
| `cite_as` | `false` | Emit `<link rel="cite-as">` in HTML. No CLI flag |
| `snapshots` | `"version-iri"` | `--snapshots` |
| `release` | none | `--release` |
| `latest` | `"release"` | Recorded in `versions.json`; changes no bytes |
| `hosts` | `[]` | `--host` |
| `pdf` | `false` | `--pdf` |
| `previous` | `[]` | `--previous` |
| `md_frontmatter` | `"none"` | `--md-frontmatter` |
| `link_style` | `"iri"` | `--link-style` |
| `base_path` | none | `--base-path`; affects `404.html` only |
| `external_paths` | `[]` | Path prefixes on this origin that belong to another application, so the link auditor does not resolve them against this build |
| `theme_switch` | `false` | `--theme-switch`. Adds two inline `<script>` blocks per page; no `.js` file is written, and a build without it emits no script other than the JSON-LD block |
| `color_scheme` | `"auto"` | `--color-scheme`; `auto`, `light`, `dark` |

`[namespaces."<namespace IRI>"]`, the only per-namespace knobs:

| Key | Default | Effect |
| --- | --- | --- |
| `mount` | path part of the namespace IRI | Directory under the site root; normalised to end in `/` |
| `resolver_prefix` | none | Copied into `manifest.json` |
| `url_style` | `"flat"` | `"dir"` switches the whole namespace to `<Local>/index.<ext>` |
| `stem` | preferred prefix, else last mount segment, else `vocabulary` | Basename of the whole-namespace serialisations |

`[llms]`: `max_terms` (default `500`), `data_site` (default none).

Top-level `inputs`, `examples`, `narrative`, plus the tables above. **`examples` and `narrative` are accepted but read nowhere**.

**Environment variables** accepted by `build`: `IYO_OUT`, `IYO_BASE_URL`, `IYO_CONFIG`, `IYO_THEME`, `IYO_TYPST_BIN`, `IYO_COLOR_SCHEME`, `IYO_BASE_PATH`, `IYO_LINK_STYLE`, `IYO_MD_FRONTMATTER`.

## Build-time checks that constrain the layout

`iyo check` runs standalone and also inside `build`. A build refuses to write when any check is an error.

| Rule id | Level | Condition |
| --- | --- | --- |
| `site.reserved-path-collision` | **error** | A local name equals a reserved path segment of its namespace (a nested document IRI or a release segment). A term `ex:shapes` in a namespace that also publishes `https://example.org/vocab/shapes/` fails the build with exit code 1 |
| `site.case-collision` | warning | Two local names in one namespace differ only by case. The message names both and says which one moves to a directory URL. The build proceeds |
| *(none)* | — | A local name equal to a file stem the site owns (`index`, or `404` at the site root) is handled silently: the term moves to the directory layout and appears in `dir_terms` with no diagnostic |

Other checks, not layout-related: `release.part-no-status`, `release.part-no-version-iri`, `release.prefix-undeclared`, `release.prefix-unused`, `release.has-part-missing`, `release.part-no-is-part-of`, `term.no-is-defined-by`, `term.no-definition`, `term.no-label`, `text.count-mismatch`, `header.missing`.

`--strict` on `build` refuses to write when there are warnings as well as errors.

Every build also audits the HTML and Markdown it just rendered for broken internal links and structure, writing the result to `a11y/structure.json`. The summary line reports `N pages audited, E errors, W warnings`. The bundled theme's colour pairs are checked and the result written to `a11y/contrast.json`. Only the schemes the build actually publishes (`site.color_scheme`) can stop a build, the others are reported as a note.

## Normative requirement versus this implementation's choice

This document is written so another implementation could follow it. The parts a second implementation must match are narrower than the parts iyo happens to do.

**Normative parts another implementation must match to interoperate:**

- The four per-term representations and their suffixes: `""` / `.md` / `.ttl` / `.jsonld`, mapping to `text/html`, `text/markdown`, `text/turtle`, `application/ld+json`.
- The two layouts and the URL each produces (`B/L` + `B/L.ext`; `B/L/` + `B/L/index.ext`), and that a term whose name collides with a file stem the site owns, or with another term modulo case, falls back to the directory layout and is listed in `dir_terms`.
- The identity IRI is never rewritten, and the document base is declared rather than derived.
- The negotiation algorithm as enumerated above, including: explicit types beat wildcards regardless of q; the RDF fallback; never 406; 200 for the default type and `status_code` otherwise; `Vary: Accept` on every negotiated response.
- The four signposting relations and which of them are absolute (`canonical`, `cite-as`) versus relative (`alternate`, `describedby`).
- `manifest.json`'s field names and semantics, in particular that `representations` is ordered, that `terms` is exhaustive under `resolver_type: "strict"`, that `reserved` lists non-term segments, and that `dir_terms` overrides `layout` per term.
- `llms.txt` v2 structure: H1, blockquote, H2 file lists, `Optional` last.
- Version IRI resolution: `owl:versionIRI` equal to `<doc_base><segment>/` makes the version IRI resolve to the snapshot.

**This implementation's choices, where a conforming implementation may differ:**

- `"convention": "iyo/1"` as the identifier string, and `generator` as an object rather than a string.
- The manifest filename `manifest.json`.
- `default_type` is always `text/html`, `status_code` always `303`, `cache_max_age` always `86400`, `resolver_type` always `"strict"`, none of them configurable in this version.
- The declaration order `text/html, text/markdown, text/turtle, application/ld+json`, which decides the RDF fallback target (Turtle).
- Cache-Control values `public, max-age=300, must-revalidate` (latest) and `public, max-age=31536000, immutable` (snapshot).
- Alias table extras (`markdown`, `turtle`, `xml`) beyond the portable set.
- The `<stem>` naming rule, the `_index.md` rename under `--md-frontmatter hugo`, the `iyo-` prefix on section anchor ids, `<prefix>_<local>` for foreign-term anchors.
- The version-segment grammar (`looks_like_a_version`), the `version-iri` / `all` / `none` policy names, and skipping rather than renaming on a segment/term clash.
- Everything under `a11y/`, `versions.json`, `release.ttl`/`release.jsonld`, `llms-full.txt`, and the set of host adapters.
- `site.cite_as` defaulting to `false`.
