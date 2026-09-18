# Findings and rule ids

[README](../README.md) · [cli](cli.md) · **rules** · [theming](theming.md) · [output-convention](output-convention.md) · [ci](ci.md)

Every finding the tool can report, by id.

Rule ids are a public interface. Ids may be added, and they are not renamed, so
a `--select` or `--ignore` written against this page keeps working.

## Which commands emit finding ids, and where they land

There is one id namespace but five producers.

| prefix family | ids | produced by | printed where | machine-readable at |
| --- | --- | --- | --- | --- |
| `release.*` | 8 | `iyo check`, and the same check re-run inside `iyo build` | stdout (check), stderr (build) | `iyo check --json` |
| `header.*` | 1 | `iyo check`, `iyo build` | stdout (check), stderr (build) | `iyo check --json` |
| `term.*` (lint subset) | 6 | `iyo check`, `iyo build` | stdout (check), stderr (build) | `iyo check --json` |
| `site.*` | 2 | `iyo check`, `iyo build` | stdout (check), stderr (build) | `iyo check --json` |
| `text.*` | 2 | `iyo check`, `iyo build` | stdout (check), stderr (build) | `iyo check --json` |
| `a11y.*` | 22 | `iyo build` page audit | stderr | `<out>/a11y/structure.json` |
| `html.*` | 2 | `iyo build` page audit | stderr | `<out>/a11y/structure.json` |
| `markdown.*` | 2 | `iyo build` Markdown-sibling audit | stderr | `<out>/a11y/structure.json` |
| `theme.*` | 6 | `iyo build` contrast gate | stderr (as failing pairs, not as ids) | `<out>/a11y/contrast.json` |
| `probe.*` | 11 | `iyo probe` | stderr | `iyo probe --json` |
| `term.*`, `constraint.*`, `document.*` (change subset) | 24 | `iyo diff`, and the changelog `iyo build --previous` writes | stdout | `iyo diff --json` |

`iyo conform` emits no finding ids at all: it reports named cases and a pass or fail per FOOPS probe.

`term.*` is split across two producers. `iyo check` emits only `term.no-label`, `term.no-definition`, `term.no-is-defined-by`, `term.deprecated-without-replacement`, `term.duplicate-label`, `term.untagged-literal`. `iyo diff` emits only the other thirteen.

Total: **86 ids**.

## `iyo check` rules — all 19

Severity is fixed per rule except `header.missing`, which has two tiers (next section).

| id | severity | what it means for a publisher |
| --- | --- | --- |
| `release.no-document` | error | No `owl:Ontology` and no `skos:ConceptScheme` was found in the inputs. Terminal for its family: the `release.*` and `header.*` checks stop here, and nothing about parts, versions or root metadata is reported. |
| `release.prefix-unused` | warning | A file declares a prefix whose namespace appears in no triple anywhere in the release. Reported per file, per prefix. |
| `release.prefix-undeclared` | warning | This file uses a namespace that the release names with a prefix somewhere else, but this file does not declare that prefix. Only applied to files parsed as `turtle`, `trig`, `n3`, `rdfxml` or `jsonld`. |
| `release.has-part-missing` | warning | A document in the inputs is not listed in the release root's `dcterms:hasPart`. |
| `release.part-no-is-part-of` | warning | A non-root document has no `dcterms:isPartOf` pointing back at the root. |
| `release.part-no-version-iri` | warning | A non-root document has no `owl:versionIRI`; it inherits the root's version. |
| `release.part-no-status` | warning | A non-root document has no `adms:status`; it inherits the root's status. |
| `release.empty-document` | info | A document typed `owl:Ontology` declares no terms of its own and reuses none. |
| `header.missing` | warning or info | One metadata property is absent from the release root. One finding per property; see the next section for the list and the two tiers. |
| `term.no-label` | error | A local term has no label. |
| `term.no-definition` | error | A local term has no definition. |
| `term.no-is-defined-by` | warning | A local term has no `rdfs:isDefinedBy` naming its vocabulary. |
| `term.deprecated-without-replacement` | warning | A deprecated term carries no `dcterms:isReplacedBy`, so a consumer is told to stop using it and not what to use instead. |
| `term.duplicate-label` | warning | Two or more local terms carry the same label, compared lowercased, across the whole release (not per namespace). One finding per colliding label; the message quotes the lowercased form and lists every IRI. |
| `term.untagged-literal` | info | A local term has a label or a definition with no language tag. Emitted only under `--strict-lang`, and at most one finding per term (it stops at the first untagged literal). |
| `site.reserved-path-collision` | error | A term's local name is identical to a path segment the site reserves inside that namespace. Reserved segments are the single-segment tails of nested document IRIs and of version IRIs under the namespace. |
| `site.case-collision` | warning | Two or more local names in one namespace differ only by case, so they are one file on a case-insensitive filesystem. Reported for every member of the group except the first in byte order; those are published at a directory URL instead, so nothing is lost. |
| `text.count-mismatch` | warning | The root's `dcterms:abstract` or `dcterms:description` says "<number word> classes" and the release declares a different number of classes. Number words `one` through `ten` only; matched case-insensitively. |
| `text.mentions-absent-vocab` | warning | The root's abstract or description contains `DOAP`, `ADMS`, `DCAT` or `PROV` as a substring and the release declares no prefix for the matching namespace (`http://usefulinc.com/ns/doap#`, `http://www.w3.org/ns/adms#`, `http://www.w3.org/ns/dcat#`, `http://www.w3.org/ns/prov#`). Substring match, so a word that merely contains one of those four sequences will match. |

Output order is severity descending (error, warning, info), then rule id ascending, then subject ascending.

Text line format: `{file-or-subject-or-dash}  {severity:<7}  {message}  [{rule}]`.

## `header.missing`: the fifteen properties and the two severity tiers

One finding per absent property, all under the single id `header.missing`. The property name is in the message, not in the id.

**Warning tier** (three properties):

| property | message |
| --- | --- |
| `dcterms:title` | `release root has no dcterms:title` |
| `dcterms:description` | `release root has no dcterms:description` |
| `dcterms:license` | `release root has no dcterms:license` |

**Info tier** (twelve properties), reported in this order:

| # | property |
| --- | --- |
| 1 | `dcterms:creator` |
| 2 | `owl:versionIRI` |
| 3 | `owl:versionInfo` |
| 4 | `vann:preferredNamespacePrefix` |
| 5 | `vann:preferredNamespaceUri` |
| 6 | `dcterms:created` |
| 7 | `dcterms:modified` |
| 8 | `dcterms:issued` |
| 9 | `dcterms:bibliographicCitation` |
| 10 | `dcterms:identifier` |
| 11 | `owl:priorVersion` |
| 12 | `dcterms:contributor` |

`owl:priorVersion` is conditional: it is only reported absent when `owl:versionInfo` is present.

Checked on the release root only, never on part documents.

## Page and Markdown audit rules — all 26 (`a11y.*`, `html.*`, `markdown.*`)

Run by `iyo build` over every `.html` and `.md` file it produced, before writing. Levels are `error` and `warning` only, with no info level. The `criterion` column is the exact string the tool prints and stores. It names the WCAG success criterion or the axe/html-validate rule the check stands in for.

| id | level | criterion | what it means |
| --- | --- | --- | --- |
| `a11y.no-html-element` | error | `parsing` | No `html` element was found on the page. |
| `a11y.no-lang` | error | `WCAG 3.1.1` | The `html` element has no `lang` attribute. |
| `a11y.lang-invalid` | error | `WCAG 3.1.1` | The `html` `lang` value is not a well-formed language tag (subtags of 1–8 ASCII alphanumerics). |
| `a11y.lang-part-invalid` | error | `WCAG 3.1.2` | A `lang` on a non-`html` element is not well formed. |
| `a11y.charset-late` | error | `HTML validity` | No character encoding declaration in the first 1024 bytes. |
| `a11y.no-viewport` | error | `WCAG 1.4.10` | No `meta name="viewport"`, so the page cannot reflow. |
| `a11y.no-title` | error | `WCAG 2.4.2` | The page has no `title` element. |
| `a11y.missing-landmark` | error | `axe region, WCAG 1.3.1` | A `header`, `main` or `footer` landmark is absent. One finding per missing landmark. |
| `a11y.multiple-main` | error | `axe landmark-one-main` | More than one `main` landmark. |
| `a11y.unnamed-nav` | error | `axe landmark-unique` | A page with several `nav` landmarks does not name each one with `aria-label` or `aria-labelledby`. |
| `a11y.no-h1` | error | `WCAG 1.3.1` | The page has no `h1`. |
| `a11y.multiple-h1` | warning | `axe page-has-heading-one` | The page has more than one `h1`. |
| `a11y.heading-skip` | error | `axe heading-order, WCAG 1.3.1` | A heading level jumps by more than one. Reported once per page (it stops at the first jump). |
| `a11y.empty-id` | error | `HTML validity` | An element has `id=""`. |
| `a11y.invalid-id` | error | `HTML validity, html-validate valid-id` | An `id` contains a colon or a space, which breaks a fragment reference. |
| `a11y.duplicate-id` | error | `axe duplicate-id, WCAG 4.1.1` | The same `id` appears more than once. |
| `a11y.dangling-fragment` | error | `WCAG 2.4.4` | An `a href="#x"` points at an id nothing on the page has. |
| `a11y.empty-link` | error | `axe link-name, WCAG 2.4.4` | A link has no text and no `aria-label`, `aria-labelledby` or `title`. |
| `a11y.broken-internal-link` | warning | `link integrity` | A link resolves inside the site but no file was produced at that address. Suppressed for paths declared in `site.external_paths`. Four candidates are tried: the target, `target.html`, `targetindex.html`, `target/index.html`. |
| `a11y.table-no-caption` | warning | `WCAG 1.3.1, technique H39` | A `table` has no `caption`. |
| `a11y.table-no-headers` | error | `axe td-has-header, WCAG 1.3.1` | A `table` has no `th` cells. |
| `a11y.th-no-scope` | warning | `WCAG 1.3.1, technique H63` | A `th` has no `scope` attribute. Only checked when the table has header cells. |
| `html.absolute-nav-link` | error | `link integrity` | An `a href` is an absolute URL under `base_url` pointing at a page this build wrote, which ties the output to one origin. Not raised for paths declared in `site.external_paths`, and never for `link rel="canonical"` or `cite-as`, which are `link` elements. |
| `html.escaped-markup` | error | `template correctness` | One of `&lt;a `, `&lt;code`, `&lt;span`, `&lt;p&gt;`, `&lt;div`, `&lt;strong` appears as visible text outside a `pre` block, meaning a template escaped markup it meant to emit. At most one finding per opener per page. |
| `markdown.absolute-link` | error | `link integrity` | A `](target)` in a Markdown sibling starts with `base_url`. Bare URLs in prose are not flagged. |
| `markdown.broken-link` | warning | `link integrity` | A relative `](target)` in a Markdown sibling has no file in the output. |

Stderr formats: errors print as `{page}  error    {message}  [{rule}, {criterion}]`, capped at the first 20. Warnings print grouped, one line per rule: `warning  {rule}: {count} across the site, e.g. {page}: {example}`.

**Error level stops the build. Warning level does not, including under `--strict`.** `--strict` on `build` gates the RDF `check` warnings only.

## Theme contrast gate — `theme.*`

Run by `iyo build` over the merged token set, before any HTML is written. Severities use the same three-value enum as `check` (`error`, `warning`, `info`), serialised lowercase.

| id | severity | what it means | reachable from the CLI? |
| --- | --- | --- | --- |
| `theme.contrast-text` | error | A pair whose basis is WCAG 1.4.3 body text is below 4.5:1. | yes |
| `theme.contrast-non-text` | error | A pair whose basis is WCAG 1.4.11 non-text (focus rings, borders, badge outlines) is below 3:1. | yes |
| `theme.contrast-link-vs-text` | error | A link colour against body text is below 3:1 (WCAG 1.4.1, technique G183). Only checked when `links.underline = false`. | yes |
| `theme.contrast-large-text` | error | A pair whose basis is the WCAG 1.4.3 large-text exception is below 3:1. | **no** |
| `theme.token-syntax` | error | A colour token is not a parseable `#rrggbb` value. That whole scheme is then skipped, so its pairs are not checked at all. | yes |
| `theme.token-schema` | error | The token set declares a schema string this build does not read. | **no** |

The gate checks 28 required pairs in each of the two schemes, plus 2 conditional pairs per scheme when links are not underlined: 56 pair results with the default tokens, 60 with underlines off. Thresholds: 4.5:1 for body text, 3:1 for large text, non-text and link-vs-text. The formula string the report carries is `WCAG 2.2 relative luminance, (L1 + 0.05) / (L2 + 0.05)`.

Only failures in a **published** scheme stop the build. `site.color_scheme` decides which schemes are published. Failures in an unpublished palette are still computed and reported as a build note.

On failure `iyo build` prints the failing pairs, not the ids: `{scheme} {fg} on {bg} is {ratio}:1, needs {required}:1  ({site})` plus the two hex values, capped at 20. The ids appear in `<out>/a11y/contrast.json`, which is only written when the build completes.

## `iyo probe` findings — all 11

Emitted against a live origin. Levels are `error` and `warning`. The `Level::Info` variant exists in the type but no rule produces it. The `foops` column is the FOOPS! probe the finding rolls up into, carried in the JSON as `foops`.

| id | level | foops | what it means |
| --- | --- | --- | --- |
| `probe.unreachable` | error | `URI1` | A request got no response at all. |
| `probe.host-unreachable` | error | `URI1` | Every request in the run failed to reach the origin. Replaces all the individual `probe.unreachable` findings with one, but only when the run made at least 2 requests and *all* of them failed. A host that is up with some IRIs missing still reports them one by one. |
| `probe.no-page` | error | `URI1` | A page request answered a status other than 200. |
| `probe.wrong-type` | error, or warning for an optional file | `URI1` for a page; `RDF1` for a required file; none for an optional file | The response's content type is not the one that was asked for. |
| `probe.no-representation` | error | `CN1` when the media type contains `markdown`, else `RDF1` | A representation request answered an unexpected status, or a required file answered a status other than 200. |
| `probe.missing-file` | warning | none | An optional file answered a status other than 200. |
| `probe.redirect-without-location` | error | `CN1` | A 301–308 answer carried no `Location` header. |
| `probe.wrong-redirect` | error | `CN1` | A 301–308 answer pointed somewhere other than the expected representation URL (compared ignoring a trailing slash). |
| `probe.not-negotiated` | error | `CN1` | A 200 answer to a negotiated request served a different media type than the one requested. |
| `probe.no-vary` | warning | `CN1` | A negotiated answer carried no `Vary: Accept`, which is a cache-poisoning risk. |
| `probe.no-signpost` | warning | none | A response has no `rel="cite-as"` link, or no `rel="describedby"` link. One finding per missing relation. |

Finding JSON fields: `rule`, `level`, `foops`, `url` (redacted), `accept`, `message`.

## `iyo diff` change ids — all 24

A different severity vocabulary from the lint rules: `breaking`, `additive`, `editorial`. These are not exit-code tiers. See the exit-code section.

**Always breaking**

| id | what it means |
| --- | --- |
| `term.removed` | A term that was published is gone. |
| `term.kind-changed` | A term changed kind (a class became a property, and so on), invalidating every use of it. |
| `term.parent-removed` | A term lost a parent, so entailments that data relied on are gone. |
| `term.domain-changed` | A property's `rdfs:domain` changed. |
| `term.range-changed` | A property's `rdfs:range` changed. |
| `constraint.now-required` | A shape made a property required; records that were valid are rejected. |
| `constraint.scheme-changed` | The concept scheme a shape draws values from changed. |
| `constraint.added` | A shape now constrains a property it did not constrain before. |
| `document.removed` | A whole document is gone. |

**Breaking or additive, decided by direction**

| id | breaking when | additive when |
| --- | --- | --- |
| `constraint.cardinality-changed` | narrower: the ceiling dropped or the floor rose | otherwise |
| `constraint.datatype-changed` | fewer datatypes than before | otherwise |
| `constraint.values-changed` | fewer values in the `sh:in` enumeration than before | otherwise |

**Always additive**

| id | what it means |
| --- | --- |
| `term.added` | A new term. |
| `term.deprecated` | A term was deprecated. Additive, not breaking: it still resolves. |
| `term.undeprecated` | A deprecation was withdrawn. |
| `term.defined` | A term that had no definition now has one. |
| `term.parent-added` | A term gained a parent. |
| `constraint.removed` | A shape no longer constrains a property. |
| `constraint.now-optional` | A required property became optional. |
| `document.added` | A new document. |

**Always editorial**

| id | what it means |
| --- | --- |
| `term.relabelled` | The label changed. |
| `term.redefined` | The definition text changed. |
| `term.definition-removed` | The definition was removed. |
| `document.version-changed` | The document's `owl:versionInfo` changed. |

Change JSON fields: `rule`, `severity`, `iri`, `label`, `namespace`, `anchor`, `detail`. The report also carries `counts` (per rule) and `breaking` (a count). Changes sort by severity, then rule, then IRI, then detail. A rule id is turned into a section heading by capitalising and replacing the dot: `constraint.values-changed` becomes "Constraint values changed".

## How `--select` and `--ignore` match

Both flags exist on `iyo check` **only**. No other subcommand has them, and no `iyo.toml` key sets them.

**Matching is a plain string prefix on the rule id, `rule.starts_with(value)`. It is not exact, family or substring matching.**

| value passed | matches | result on `testdata/mini` |
| --- | --- | --- |
| `term.no-is-defined-by` | that one rule | `term.no-is-defined-by: 5` |
| `release.prefix` | `release.prefix-unused`, `release.prefix-undeclared` | both, nothing else |
| `term` | every `term.*` rule | `term.no-is-defined-by: 5` |
| `rel` | every `release.*` rule (an arbitrary prefix works) | the six that fire there |
| `prefix` | nothing — the id does not *start* with it | usage error, exit 2 |
| `a11y` | nothing — `check` has no `a11y.*` rules | usage error, exit 2 |
| `nosuch.rule` | nothing | usage error, exit 2, `no rule matches "nosuch.rule"` |

Other behaviour:

- **Both forms of repetition work and are equivalent.** `--select a,b` (comma-delimited) and `--select a --select b` (repeated) produce the same result.
- **`--ignore` wins over `--select`.** Select is applied first, so a rule matching both is dropped. `--select release --ignore release.part` yields `release.has-part-missing`, `release.prefix-undeclared`, `release.prefix-unused`.
- **`--ignore` reports what it hid.** The run prints `N findings suppressed by --ignore` so a configuration that quietly stopped failing is visible.
- **A filter matching no rule is a usage error.** `--select header.missingg` exits 2 and suggests the id that was meant, rather than checking nothing and exiting 0. A family prefix such as `term.` is a legitimate filter and is accepted.
- **Filtering happens before counting.** Suppressed findings are absent from `summary.errors`/`warnings`/`info` and from `summary.by_rule`.
- **`--ignore` can therefore change the exit code**, including hiding error-level rules: `iyo check <broken> --ignore term.no-label,term.no-definition,site.reserved` exits 0 on input that otherwise exits 1.
- Neither flag reaches the copy of `check` that runs inside `iyo build`: the build calls it with default options.

## What `--strict` and `--strict-lang` change

They are two unrelated flags with confusingly similar names.

**`--strict`** (on `check` and on `build`) changes only *when the command fails*. It never changes which rules run.

| command | without `--strict` | with `--strict` |
| --- | --- | --- |
| `iyo check` | exit 1 when `errors > 0` | exit 1 when `errors > 0` **or** `warnings > 0` |
| `iyo build` | refuses to build when the RDF check reports errors | refuses when it reports errors **or** warnings |

Side effect on `build` only: when the gate fires, `--strict` also makes it print every finding rather than just the error-level ones, `info` findings included.

**Info-level findings never affect any exit code**, under `--strict` or not. A run with 0 errors, 0 warnings and 4 info findings exits 0 under `--strict`.

**`--strict` does not gate the page audit.** It changes nothing about which `a11y.*`, `html.*` or `markdown.*` findings stop a build.

**`--strict-lang`** (on `check` only) turns on exactly one rule: `term.untagged-literal`, at info level. Because the rule is info-level, turning it on cannot by itself fail a run. It is off by default, and the copy of `check` inside `build` never sets it.

## Severity levels and exit codes

Three severity vocabularies are in play.

| producer | levels (JSON values) |
| --- | --- |
| `check`, `theme` | `error`, `warning`, `info` |
| page/Markdown audit | `error`, `warning` |
| `probe` | `error`, `warning` |
| `diff` | `breaking`, `additive`, `editorial` |

**Process exit codes** (the whole set, not only the lint ones):

| code | meaning |
| --- | --- |
| 0 | success; nothing at error level |
| 1 | findings at error level, or warnings under `--strict`, or a gate that failed |
| 2 | usage error (emitted by the argument parser) |
| 3 | input could not be read or parsed |
| 4 | a required external program is missing |
| 5 | output path conflict or I/O failure |
| 130 | interrupted |

**Which findings produce exit 1, per command:**

| command | exit 1 when |
| --- | --- |
| `iyo check` | `errors > 0`, or `--strict` and `warnings > 0`. Info is never counted. |
| `iyo build` | the RDF check reports `errors > 0` (or warnings under `--strict`); **or** the page audit reports any error-level issue; **or** any contrast pair fails in a published scheme. Page-audit warnings never fail a build. |
| `iyo probe` | default (`--fail-on error`): `errors > 0` or the run was truncated. `--fail-on warning`: errors, warnings, or truncation. `--fail-on never`: never. An interrupted run exits 130 instead. |
| `iyo conform` | `--fail-on error` and `--fail-on warning` behave identically (there is no warning tier): non-conformant exits 1. `--fail-on never` never fails. Interrupted exits 130. |
| `iyo diff` | only with a flag: `--exit-code` (anything changed) or `--fail-on-breaking` (a breaking change). Without either, a diff with breaking changes still exits 0. |

Exit codes: `check testdata/mini` → 0; `--strict` → 1; broken fixture → 1; `--ignore` over the error rules → 0; `build --strict` on mini → 1; `build` with a failing theme → 1; `build` with a malformed colour token → 1; `probe` against a wrong origin → 1, with `--fail-on never` → 0; `diff` plain → 0, `--exit-code` → 1, `--fail-on-breaking` → 1.

## Where findings are written, and in what shape

Every JSON document carries `schema_version`, currently `0.1`.

**`iyo check --json`**, one object:

```
{ "schema_version": "0.1",
  "findings": [ { "rule", "severity", "message", "subject"?, "file"? } ],
  "summary": { "errors", "warnings", "info", "by_rule": { "<id>": count } } }
```

`subject` and `file` are omitted when absent. `term.duplicate-label` carries a `subject` (the first IRI) but no `file`. Findings are sorted severity-descending, then rule, then subject.

**`<out>/a11y/structure.json`**, written by `iyo build` when the build completes:

```
{ "schema_version", "generator", "pages_checked", "errors", "warnings",
  "issues": [ { "rule", "level", "message", "page", "criterion" } ] }
```

**`<out>/a11y/contrast.json`**, written by `iyo build` when the build completes:

```
{ "schema_version", "formula", "theme", "underlined_links",
  "pairs": [ { "scheme", "foreground", "background", "fg_value", "bg_value",
               "ratio", "ratio_2dp", "required", "basis", "conditional",
               "passes", "site" } ],
  "findings": [ { "rule", "severity", "message", "subject"?, "file"? } ],
  "failed" }
```

`basis` is serialised kebab-case: `body-text`, `large-text`, `non-text`, `link-vs-text`. `file` on a theme finding is the literal string `tokens.toml`.

Because a contrast pair failure stops the build, and the two `a11y/` files are written by the build, a `theme.contrast-*` finding never appears in a `contrast.json` on disk. The failing pairs are printed to stderr instead. `theme.token-syntax` is the exception: it does not stop the build, so it does reach `contrast.json`.

**`iyo probe --json`**: `{ schema_version, origin, requests, made, truncated, findings, errors, warnings, foops, foops_status }`. Each finding is `{ rule, level, foops, url, accept, message }`.

**`iyo diff --json`**: `{ schema_version, scope, old, new, breaking, counts, changes }`. Each change is `{ rule, severity, iri, label, namespace, anchor, detail }`.

## Complete id index (86 ids, alphabetical within family)

Grouped by prefix family, with the command that emits each.

**`a11y.` (22): `iyo build`**
`a11y.broken-internal-link`, `a11y.charset-late`, `a11y.dangling-fragment`, `a11y.duplicate-id`, `a11y.empty-id`, `a11y.empty-link`, `a11y.heading-skip`, `a11y.invalid-id`, `a11y.lang-invalid`, `a11y.lang-part-invalid`, `a11y.missing-landmark`, `a11y.multiple-h1`, `a11y.multiple-main`, `a11y.no-h1`, `a11y.no-html-element`, `a11y.no-lang`, `a11y.no-title`, `a11y.no-viewport`, `a11y.table-no-caption`, `a11y.table-no-headers`, `a11y.th-no-scope`, `a11y.unnamed-nav`

**`constraint.` (8): `iyo diff`**
`constraint.added`, `constraint.cardinality-changed`, `constraint.datatype-changed`, `constraint.now-optional`, `constraint.now-required`, `constraint.removed`, `constraint.scheme-changed`, `constraint.values-changed`

**`document.` (3): `iyo diff`**
`document.added`, `document.removed`, `document.version-changed`

**`header.` (1): `iyo check`**
`header.missing`

**`html.` (2): `iyo build`**
`html.absolute-nav-link`, `html.escaped-markup`

**`markdown.` (2): `iyo build`**
`markdown.absolute-link`, `markdown.broken-link`

**`probe.` (11): `iyo probe`**
`probe.host-unreachable`, `probe.missing-file`, `probe.no-page`, `probe.no-representation`, `probe.no-signpost`, `probe.no-vary`, `probe.not-negotiated`, `probe.redirect-without-location`, `probe.unreachable`, `probe.wrong-redirect`, `probe.wrong-type`

**`release.` (8): `iyo check`**
`release.empty-document`, `release.has-part-missing`, `release.no-document`, `release.part-no-is-part-of`, `release.part-no-status`, `release.part-no-version-iri`, `release.prefix-undeclared`, `release.prefix-unused`

**`site.` (2): `iyo check`**
`site.case-collision`, `site.reserved-path-collision`

**`term.` (6): `iyo check`**
`term.deprecated-without-replacement`, `term.duplicate-label`, `term.no-definition`, `term.no-is-defined-by`, `term.no-label`, `term.untagged-literal`

**`term.` (13): `iyo diff`**
`term.added`, `term.defined`, `term.definition-removed`, `term.deprecated`, `term.domain-changed`, `term.kind-changed`, `term.parent-added`, `term.parent-removed`, `term.range-changed`, `term.redefined`, `term.relabelled`, `term.removed`, `term.undeprecated`

**`text.` (2): `iyo check`**
`text.count-mismatch`, `text.mentions-absent-vocab`

**`theme.` (6): `iyo build`**
`theme.contrast-large-text`, `theme.contrast-link-vs-text`, `theme.contrast-non-text`, `theme.contrast-text`, `theme.token-schema`, `theme.token-syntax`

84 of the 86 were emitted at runtime during this audit. The two that were not are `markdown.absolute-link` and `markdown.broken-link`.
