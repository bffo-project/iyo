# iyo

**iyo** is an OWL, RDFS, SKOS, and SHACL vocabulary/ontology publishing tool
that produces themeable ontology web pages, with support for additional
Markdown, Turtle, and JSON-LD. Alongside human-readable documentation, iyo
publishes LLM context files tailored to help AI agents understand the
vocabulary more effectively.

iyo focuses on the accessibility of the pages it generates, which it checks
against web accessibility standards on every build, and on integrating
seamlessly with existing CMSs such as Hugo. Its themeable output is designed to
blend in with the rest of your website.

It also writes a manifest describing the content negotiation the site needs,
and compiles that manifest into configuration for Cloudflare Workers, Apache,
Vercel, or GitHub Pages. iyo can additionally test and validate vocabulary URIs
and the content negotiation on those IRIs.

The name is 伊予 (Iyo), the historic province that is now Ehime, where the tool
was started during [BioHackathon 2026](https://2026.biohackathon.org) in Matsuyama.

## Status

Version 0.1.0, the first release. Nine commands work end to end: `check`,
`model`, `build`, `pdf`, `probe`, `conform`, `serve`, `diff` and `profiles`.
Read [Limitations](#limitations) before adopting it.

## Install

Requires a Rust toolchain, 1.88 or newer. CI checks that floor on every push.

```console
$ cargo install --locked iyo
$ iyo --version
iyo 0.1.0
```

`--locked` builds against the dependency versions this release was tested
with. Drop it to take newer compatible ones.

To work on iyo, or to run its tests, build from a checkout instead:

```console
$ git clone https://github.com/bffo-project/iyo
$ cd iyo
$ cargo build --release
$ ./target/release/iyo --version
```

Most of the tool is one binary with nothing else to install. Two commands reach
for a program on `PATH`: `probe` and `conform` need `curl` to make requests, and
`pdf` needs `typst`. Without `typst` a build still writes the site and skips the
PDF, which `iyo pdf` can compile later. `check`, `model`, `build`, `diff`,
`serve` and `profiles` need neither.

## Quick start

The examples below use the fixture in this repository, so the output is exactly
what you will see. `cargo install` gives you the binary but not the fixture:
clone the repository for that, or point the same commands at your own files.

`check` lints without writing anything. One line per finding, then a summary:

```console
$ iyo check testdata/mini
testdata/mini/shapes.ttl  warning  document is part of the release but <https://example.org/vocab/> does not list it in dcterms:hasPart  [release.has-part-missing]
testdata/mini/vocab.ttl  warning  prose says "three classes" but the release declares 2  [text.count-mismatch]
testdata/mini/vocab.ttl  info     release root has no dcterms:contributor  [header.missing]
3 documents, 10 local terms, 1 reused, 99 triples from 3 files
0 errors, 16 warnings, 6 info
```

Three of the twenty-two findings are shown. The exit status is 0 because none
of them is an error. [docs/rules.md](docs/rules.md) names every id and says what
`--strict` changes.

Then build:

```console
$ iyo build testdata/mini --out dist --base-url https://example.org/
107 files, 3 documents, 10 terms in 3 namespaces -> dist
22 pages audited, 0 errors, 0 warnings; theme contrast checked
```

Between those two lines `build` also prints a `digest`: a hash of the output
tree, identical for identical inputs, so CI can use it as a cache key. It is
left out here because it changes whenever the bundled theme does.

Every term ends up as four files sharing a stem, so a URL is predictable without
consulting an index:

```
dist/vocab/category.html     dist/vocab/category.ttl
dist/vocab/category.md       dist/vocab/category.jsonld
```

To read the result locally with negotiation in front of it:

```console
$ iyo serve dist
```

`serve` is for development only: one connection at a time, no TLS, bound to
localhost.

The site root also carries `manifest.json`, an `llms.txt` index, a JSON-LD
context, the release as a single graph, and the build's own audit results.
[docs/output-convention.md](docs/output-convention.md) describes the whole
layout, and is the page to read if you want to implement the convention
elsewhere.

## Example output

One vocabulary, built twice by the same version of iyo, differing only in
`--theme`:

- [Default theme](https://bffo-project.github.io/iyo-demo/default/)
- [BFFO theme](https://bffo-project.github.io/iyo-demo/bffo/)

The sources and the workflow that builds them are in
[bffo-project/iyo-demo](https://github.com/bffo-project/iyo-demo), which
installs iyo from crates.io rather than from a checkout.

They are served by GitHub Pages, which cannot negotiate, so they show the pages,
the four representations per term and the file layout, but not the negotiation.
`iyo conform` scores that deployment 16 of 54: every case it passes is one that
must *not* resolve, and everything requiring negotiation fails. That is the gap
the generated host configurations exist to close.

## Configuration

Flags work without a config file. For a real vocabulary the settings go in
`iyo.toml` beside the sources:

```toml
[site]
base_url = "https://example.org/"
title    = "Example Vocabulary"
```

Unknown keys are refused rather than ignored, so a misspelling is reported
instead of silently doing nothing. Every key is listed in
[docs/cli.md](docs/cli.md).

## Theming

`--theme DIR` overrides the built-in theme file by file: templates,
stylesheets, design tokens, and the Typst source for the PDF. Colour is declared
by role rather than by hue, and the build refuses to write a theme whose
contrast falls below the threshold. [docs/theming.md](docs/theming.md) is the
page to work from.

## Accessibility

The default theme is **designed to meet WCAG 2.2 Level AA**. That is a statement
of intent, not a conformance claim: conformance is a property of complete pages
in context, which a template cannot assert on its own.

Every build runs 22 structural checks over the pages it writes, records the
results in `a11y/` as data, and refuses to write when one of them reports an
error. [docs/rules.md](docs/rules.md) names each check and the WCAG criterion it
stands for.

Automated checking covers a minority of WCAG in any tool. Nothing here replaces
a keyboard walk, a zoom test and a screen-reader pass.

## Limitations

- **Most generated host configurations have not been tested on those hosts.**
  GitHub Pages is the exception, and measuring a real deployment corrected what
  the adapter claimed: term IRIs do resolve there, to HTML, which it previously
  said they did not. Cloudflare, Apache and Vercel are still only checked
  against this tool's own `serve`, on localhost.
- **No starting configuration ships.** There is no `iyo init` and no example
  `iyo.toml` beyond the fragment above.
- **PDF output depends on the Typst version.** Below Typst 0.15 the PDF is
  PDF/A-2a; PDF/UA-1 is requested only at 0.15 and above.
- **Built and tested on macOS on aarch64 only.** Linux is covered by CI and
  nothing else has been tried.
- **`--link-style` decides whether the site opens from `file://`.** The default
  writes real IRIs, which need a server. `--link-style file` writes relative
  paths for local browsing.
- **Pages carry one `<script type="application/ld+json">` block.** No executable
  JavaScript is emitted unless `--theme-switch` is passed.

## Documentation

**To look something up.** [docs/cli.md](docs/cli.md) for every command, flag,
exit code and environment variable. [docs/rules.md](docs/rules.md) for every
finding id, and for what `--select`, `--ignore` and `--strict` do.
[docs/theming.md](docs/theming.md) for the override surface, template contexts
and design tokens.

**To do something.** [docs/ci.md](docs/ci.md) walks through using iyo as a gate
in a pipeline: what to run on a pull request, before publishing, and against the
live site afterwards.

**To understand the layout, or implement it elsewhere.**
[docs/output-convention.md](docs/output-convention.md) is the file and URL
convention, the negotiation rules and the manifest, written so another tool
could follow it.

If a build refuses to write and you want to know why, start with
[docs/rules.md](docs/rules.md), then [docs/theming.md](docs/theming.md) if the
message names a colour pair.

## Licence

Apache-2.0. See [LICENSE](LICENSE).

One exception: [docs/output-convention.md](docs/output-convention.md) is also
available under CC-BY-4.0, on the terms stated at the top of that page.

## Authors

Nishad Thalhath and Kozo Nishida.
