# Command-line reference

[README](../README.md) · **cli** · [rules](rules.md) · [theming](theming.md) · [output-convention](output-convention.md) · [ci](ci.md)

Every command, flag, exit code and environment variable, as the binary accepts
them.

Seven subcommands named in the design notes are not implemented and are not
documented here.

## Invocation, commands, and version

Binary name `iyo` (`Cargo.toml` `[[bin]] name = "iyo"`). Version reported by the binary under test: `iyo 0.1.0`.

```
Usage: iyo [OPTIONS] <COMMAND>
```

Nine subcommands, plus clap's built-in `help`:

| Command | One-line meaning (verbatim from `iyo --help`) |
|---|---|
| `check` | Lint the RDF of a release without writing anything |
| `model` | Print the intermediate model as JSON |
| `build` | Render the site into an output directory |
| `pdf` | Compile the PDF of each namespace from a built directory |
| `probe` | Probe every IRI on a live origin: exhaustive, minutes |
| `conform` | Gate a live origin on the negotiation convention: fixed cases, seconds |
| `serve` | Serve a built site, honouring the manifest's negotiation |
| `diff` | Compare two releases and report what changed |
| `profiles` | List the profiles this build knows about |
| `help` | Print this message or the help of the given subcommand(s) |

Behaviours worth knowing:

- `iyo` with no subcommand prints the short help and **exits 2**, not 0.
- `-h` prints a one-screen summary; `--help` prints the long form with a per-command Examples block above `Usage:` and a `Documentation: https://github.com/bffo-project/iyo` trailer below the flag table.
- `iyo help <command>` works and exits 0.
- `-V` / `--version` exist **only on the top-level command**. `iyo check -V` fails with `error: unexpected argument '-V' found`, exit 2.
- A mistyped subcommand gets a did-you-mean: `iyo buld` → `error: unrecognized subcommand 'buld'` + `tip: a similar subcommand exists: 'build'`, exit 2.

## The six global flags

These appear on every subcommand's help screen as well as the top-level one.

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--json` | — | none | — | off | Print machine-readable JSON on stdout |
| `--quiet` | `-q` | none | — | off | Suppress the summary on stderr |
| `--no-color` | — | none | — | off | Never colour the output. The same as `--color never` |
| `--color` | — | `<WHEN>` | `auto`, `always`, `never` | `auto` | When to colour messages: auto (a terminal), always, or never |
| `--debug` (visible alias `--verbose`) | `-d` | none | — | off | Explain each step on stderr as it happens |
| `--help` | `-h` | none | — | — | Print help (`-h` short form, `--help` long form) |

Mechanics:

- Five of the six (`--json`, `--quiet`, `--no-color`, `--color`, `--debug`) are declared `global = true`. `-h`/`--help` is added by clap to every command. They may be given **before or after** the subcommand: `iyo --json profiles` and `iyo profiles --json` both work, as do `iyo -d check testdata/mini` and `iyo check testdata/mini --debug`.
- `--color=always` (equals form) is accepted as well as `--color always`.
- `--verbose` is accepted as an alias for `--debug`.
- `--debug` output is prefixed `debug:` on stderr and, for the input-reading commands, lists each file read, its triple count and detected format, then namespace/document/term counts:
  ```
  debug: reading testdata/mini/scheme.ttl
  debug: 29 triples from testdata/mini/scheme.ttl as turtle
  debug: 3 namespaces, 3 documents, 11 terms
  ```
- `-V`/`--version` is **not** global (see previous section).

## Colour: precedence

The decision is taken about **stderr**, which is the only stream that ever carries colour (the tool's own messages and clap's usage errors). Order, first match wins:

| # | Condition | Result |
|---|---|---|
| 1 | `--no-color` given, or `--color never` | colour **off** |
| 2 | `--color always` | colour **on** |
| 3 | `NO_COLOR` set (any value, including empty) | colour **off** |
| 4 | `TERM=dumb` | colour **off** |
| 5 | `CLICOLOR_FORCE` or `FORCE_COLOR` set, non-empty and not `0` | colour **on** |
| 6 | otherwise | on only when stderr is a terminal |

Cases worth knowing:

- `--color always` piped → ANSI present (`^[[1m^[[31merror:^[[0m …`).
- `NO_COLOR=1 --color always` → ANSI **still present** (rule 2 beats rule 3).
- `NO_COLOR=1` alone, piped → no ANSI.
- `CLICOLOR_FORCE=1` piped → ANSI present. `FORCE_COLOR=1` piped → ANSI present.
- `TERM=dumb CLICOLOR_FORCE=1` → no ANSI (rule 4 beats rule 5).
- `--no-color` → no ANSI.

## `iyo check`: flags

```
Usage: iyo check [OPTIONS] <INPUT>...
```

**Positional:** `<INPUT>...`, *required*, one or more. "Files, directories or patterns such as 'vocabularies/*.ttl'". `iyo check` with no input exits 2.

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--strict` | — | none | — | off | Exit non-zero on warnings as well as errors |
| `--strict-lang` | — | none | — | off | Report literals with no language tag |
| `--select` | — | `<RULE>` | any rule id or id prefix | none (all rules) | Only run rules whose id starts with one of these |
| `--ignore` | — | `<RULE>` | any rule id or id prefix | none | Skip rules whose id starts with one of these |

Plus the six global flags.

`--select` / `--ignore` accept **both** forms (they are comma-delimited *and* repeatable), and match on **id prefix**:

- `--select term.no-label,term.no-definition` (comma) and `--select a --select b` (repeated) produce the same two findings.
- `--select term.` selected 5 findings, the same count as `--select term.no-is-defined-by` on that fixture, i.e. prefix matching is on.
- `--ignore header.` cut a 22-line report to 16 lines.
- `--strict-lang` added 6 `term.untagged-literal` info findings (22 → 28 lines) on the same fixture.

No `--config` on `check`: `iyo check testdata/mini --config iyo.toml` → `error: unexpected argument '--config' found`.

Rule ids available to `--select`/`--ignore` (complete set in the source), in five families (`header.`, `release.`, `site.`, `term.`, `text.`):

`header.missing`, `release.empty-document`, `release.has-part-missing`, `release.no-document`, `release.part-no-is-part-of`, `release.part-no-status`, `release.part-no-version-iri`, `release.prefix-undeclared`, `release.prefix-unused`, `site.case-collision`, `site.reserved-path-collision`, `term.deprecated-without-replacement`, `term.duplicate-label`, `term.no-definition`, `term.no-is-defined-by`, `term.no-label`, `term.untagged-literal`, `text.count-mismatch`, `text.mentions-absent-vocab`.

Severity tiers are `error`, `warning`, `info`. Error-level rules in the source are `release.no-document`, `term.no-label`, `term.no-definition`, `site.reserved-path-collision`.

## `iyo model`: flags

```
Usage: iyo model [OPTIONS] <INPUT>...
```

**Positional:** `<INPUT>...`, *required*, one or more. No help text of its own in the flag table (the field carries no doc comment). `iyo model` with no input exits 2.

**No command-specific flags.** Only the six global flags.

Behaviour: `model` writes the model JSON to stdout **whether or not `--json` is given**, and the two outputs are byte-identical (`cmp` reported no difference). `-q` does not suppress it (949 stdout lines with and without `-q`, 0 stderr lines either way).

## `iyo build`: flags

```
Usage: iyo build [OPTIONS] [INPUT]...
```

**Positional:** `[INPUT]...`, *optional*. When omitted, inputs come from the config file's top-level `inputs` array. With neither, the run fails: `error: no inputs given and none configured`, exit **3**.

| Long | Short | Placeholder | Possible values | Default | Env | Meaning (verbatim) |
|---|---|---|---|---|---|---|
| `--out` (alias `--output`) | `-o` | `<DIR>` | any path | `dist` | `IYO_OUT` | Where to write. Created if missing |
| `--base-url` | — | `<URL>` | absolute http/https URL | origin of the release's root document | `IYO_BASE_URL` | Absolute URL of the site root. Defaults to the origin of the release's root document |
| `--config` | — | `<FILE>` | any path | `./iyo.toml` when it exists | `IYO_CONFIG` | Configuration file. Defaults to ./iyo.toml when it exists |
| `--strict` | — | none | — | off | — | Refuse to write when there are warnings as well as errors |
| `--theme` | — | `<DIR>` | a directory holding `templates/` and/or `assets/` | built-in theme | `IYO_THEME` | A theme directory whose files override the built-in theme one by one |
| `--snapshots` | — | `<WHEN>` | `version-iri`, `all`, `none` | `version-iri` (from the config default; **help prints no `[default:]`**) | — | When to write a versioned snapshot of a namespace: only where a version IRI points at one, for every versioned namespace, or never |
| `--release` | — | `<VERSION>` | any string | none | — | Version string to use when the RDF declares none |
| `--previous` | — | `<INPUT>` | file/dir/pattern; **repeat the flag** | none | — | The previous release, to write a changelog against. Repeatable, because a release is usually several files |
| `--pdf` | — | none | — | off | — | Write the Typst template and data for a PDF of each namespace, and compile them when `typst` is available |
| `--typst-bin` | — | `<PATH>` | any path | `typst` | `IYO_TYPST_BIN` | Where `typst` is, when it is not on PATH |
| `--host` | — | `<HOST>` | `cloudflare`, `apache`, `vercel`, `dcmi-ns`, `github-pages`; **repeat the flag** | none | — | Compile the manifest into a host's configuration. Repeatable |
| `--color-scheme` | — | `<SCHEME>` | `auto`, `light`, `dark` | `auto` (config default; **help prints no `[default:]`**) | `IYO_COLOR_SCHEME` | Which colour schemes to publish: `auto` (both, the reader's system chooses), `light` or `dark`. Forcing one is for a site that has to match surroundings offering only that one |
| `--theme-switch` | — | none | — | off | — | Offer readers a light/dark control. This is the only JavaScript this tool emits: without it a page follows the operating system's setting, which is what it does today |
| `--base-path` | — | `<PATH>` | any path | none | `IYO_BASE_PATH` | The path the site is served from, when that differs from the path in `--base-url`. Only the 404 page needs it, because it is the one page whose links cannot be relative |
| `--link-style` | — | `<STYLE>` | `iri`, `file` | `iri` (config default; **help prints no `[default:]`**) | `IYO_LINK_STYLE` | Where navigation links point: `iri` (the term IRI, the default and correct wherever negotiation is implemented) or `file` (the document that exists, for a host that serves files and nothing else). Identity is unaffected either way |
| `--md-frontmatter` | — | `<STYLE>` | `none`, `hugo`, `mkdocs`, `jekyll` | `none` (config default; **help prints no `[default:]`**) | `IYO_MD_FRONTMATTER` | Prefix per-term Markdown with YAML front matter for another site generator to consume the tree |
| `--dry-run` | `-n` | none | — | off | — | Report what would be written without writing it |

Plus the six global flags.

**Repeatability is flag-repeat only, not comma-separated**, for `--previous` and `--host`: `--host cloudflare,apache` → `error: invalid value 'cloudflare,apache' for '--host <HOST>'`, and `--previous "a,b"` is read as one path named `a,b`. Repeating both works: two `--host` flags wrote `adapters/apache/` and `adapters/vercel/`, and two `--previous` flags produced three `changes.md` files.

**Mutually exclusive:** `--theme-switch` together with `--color-scheme light|dark` is rejected, exit **2**:
```
error: --theme-switch offers a choice between schemes, and --color-scheme dark publishes only one
hint: drop one of the two: --color-scheme auto to offer both, or no --theme-switch to publish the one
```

**`--base-url` is validated.** Rejected with exit 2: not absolute http/https, no host, contains whitespace, carries `user@` credentials. Message form: `--base-url "not a url" is not an absolute http or https URL`.

**`--theme` is validated.** Two distinct exit-3 errors: the path is not a directory (`… is not a directory, so there is no theme to read`), or it is a directory holding none of `templates/`, `assets/`, `tokens.toml` or `pdf/spec.typ` (`… holds nothing a theme can override`).

## `iyo build`: what each value-taking flag does

All against the repo's `testdata/mini` (3 documents, 10 local terms, 3 namespaces).

| Flag / value | What actually changed |
|---|---|
| `--host cloudflare` | adds `adapters/cloudflare/{README.md,_headers,_redirects,worker.js,wrangler.toml}` |
| `--host apache` | adds `adapters/apache/{.htaccess,README.md}` |
| `--host vercel` | adds `adapters/vercel/{README.md,vercel.json}` |
| `--host dcmi-ns` | adds `adapters/dcmi-ns/README.md` and `adapters/dcmi-ns/resolver/*.json` (one per namespace) |
| `--host github-pages` | adds `adapters/github-pages/{.nojekyll,README.md,RESOLUTION.md}` |
| *(no `--host`)* | top level of the output: `404.html`, `context.jsonld`, `index.html`, `llms-full.txt`, `llms.txt`, `manifest.json`, `release.jsonld`, `release.ttl`, `terms.json`, `versions.json` |
| `--snapshots version-iri` | 107 files; snapshot dir `vocab/0.1.0/` written |
| `--snapshots all` | 115 files; same snapshot dir |
| `--snapshots none` | 75 files; no snapshot dirs |
| `--link-style iri` | `href="../vocab/Thing"` |
| `--link-style file` | `href="../vocab/Thing.html"` |
| `--md-frontmatter hugo` | per-term `.md` gains a `---` block with `title`, `iri`, `kind`, `weight`, `url` |
| `--md-frontmatter none` | `.md` starts directly with the `> Part of …` line |
| `--previous <dir>` | adds one `changes.md` per namespace, e.g. `vocab/changes.md`, `vocabulary/category/changes.md` |
| `--release 9.9.9` | **no effect when the RDF already declares a version** — snapshot dir stayed `0.1.0` |
| `--theme-switch` | adds no files; adds two executable `<script>` blocks per page (baseline has one `<script type="application/ld+json">`, which is data, not script); changes the digest |
| `--color-scheme auto\|light\|dark` | same file count (107), different digest for each |
| `--base-path /vocab-site/` | rewrites `404.html` links only: `href="/"` → `href="/vocab-site/"` etc. |
| `--dry-run` | prints the same summary plus `dry run: nothing was written`, listing the files it would have written and their sizes without creating any; **the digest is computed and is identical to the real build's** |
| `--pdf` (with `typst` unavailable) | site is written; exits **4**; stderr: `the site was written; the PDF was not: … is not on PATH` + `hint: compile the prepared inputs later with \`iyo pdf <out>\``. Prepared inputs left at `<namespace>/pdf/spec.typ` |

## `iyo pdf`: flags

```
Usage: iyo pdf [OPTIONS] [DIR]
```

**Positional:** `[DIR]`, *optional*, default `dist`. "The directory a build wrote, with `--pdf`".

| Long | Short | Placeholder | Possible values | Default | Env | Meaning (verbatim) |
|---|---|---|---|---|---|---|
| `--pdf-standard` | — | `<LIST>` | passed through to Typst's `--pdf-standard` | picked from the installed Typst's version | — | PDF standards to enforce. Defaults to what the installed Typst supports: `ua-1,a-2a` from 0.15, `a-2a` before it |
| `--typst-bin` | — | `<PATH>` | any path | `typst` | `IYO_TYPST_BIN` | Where `typst` is, when it is not on PATH |
| `--font-path` | — | `<DIR>` | any directory; **repeat the flag** | Typst's embedded fonts | — | A directory of fonts to use instead of Typst's embedded ones |
| `--dry-run` | `-n` | none | — | off | — | List what would be compiled without running Typst. `pdf` writes files like `build` does, so it takes the same flag |

Plus the six global flags.

Behaviour:

- With Typst 0.14.2 installed, `--pdf-standard` defaulted to `a-2a`, matching the documented `< 0.15` branch. Typst 0.15 and newer takes the `ua-1,a-2a` branch.
- Successful run reports one line per PDF on **stderr**: `pdfd/vocab/ex.pdf (89 KiB, a-2a, typst 0.14.2)`. `-q` silences it.
- A directory with no `pdf/spec.typ` exits **3**: `<dir> has no pdf/spec.typ`, hint `run \`iyo build --pdf --out <dir>\` first`.
- Missing Typst exits **4** with `error: <bin> is not on PATH. Install Typst (https://typst.app/open-source/) or pass --typst-bin`.
- `--font-path` can be repeated (two were accepted).

## `iyo probe`: flags

```
Usage: iyo probe [OPTIONS] [TARGET]
```

**Positional:** `[TARGET]`, *optional*, default `dist`. "A built directory, a manifest file, or the URL of a deployed site." When it is a directory, `manifest.json` inside it is read. When it is a URL, `<TARGET>/manifest.json` is fetched.

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--origin` | — | `<URL>` | any http/https origin | the manifest's own site root, or TARGET when it is a URL | The origin to send requests to. Defaults to the manifest's own site root, or to TARGET when it is a URL |
| `--sample` | — | `<N>` | non-negative integer | unset (every term) | Probe at most this many terms per namespace |
| `--timeout` | — | `<TIMEOUT>` | integer seconds | `15` | Seconds to wait for one request |
| `--deadline` | — | `<SECONDS>` | integer seconds | `300` | Stop the whole run after this many seconds and report what was gathered, rather than continuing against a host that may never answer |
| `--full` | — | none | — | off | Include every response in the JSON output |
| `--fail-on` | — | `<LEVEL>` | `error`, `warning`, `never` | `error` | What counts as failure |

Plus the six global flags.

Exit rule, applied after the run:

| `--fail-on` | Exits 1 when |
|---|---|
| `error` (default) | `errors > 0` **or** the run was `truncated` |
| `warning` | `errors > 0` or `warnings > 0` or `truncated` |
| `never` | never (always 0) |

An interrupt takes precedence over all three and exits **130**.

`--deadline 0` makes zero requests and exits **1** (truncation counts as failure), while `--fail-on never` against the same origin exits **0**. `--sample 0` reduced a 49-request plan to 7 (namespace-level requests only). `--full` adds a top-level `responses` array to the JSON with fields `url, accept, status, content_type, vary, link, location, cache_control, cors`.

A line goes to stderr before the first request **even under `-q`**: `probe: 119 requests planned for http://127.0.0.1:9`.

## `iyo conform`: flags

```
Usage: iyo conform [OPTIONS] [TARGET]
```

**Positional:** `[TARGET]`, *optional*, default `dist`. Same three shapes as `probe`.

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--origin` | — | `<URL>` | any http/https origin | the manifest's own site root, or TARGET when it is a URL | The origin to send requests to. Defaults to the manifest's own site root, or to TARGET when it is a URL |
| `--cases` | — | `<PATH>` | any path | none | Write the resolved cases here, for the host harnesses to run |
| `--timeout` | — | `<TIMEOUT>` | integer seconds | `15` | Seconds to wait for one request |
| `--deadline` | — | `<SECONDS>` | integer seconds | `60` | Stop the whole run after this many seconds and report what was gathered … `conform` is meant to finish in seconds, so its default is far tighter than `probe`'s |
| `--fail-on` | — | `<LEVEL>` | `error`, `warning`, `never` | `error` | What counts as failure |

Plus the six global flags.

**`--cases` has a no-network mode.** When `--cases` is given, `--origin` is **not** given, and TARGET is **not** a URL, `conform` resolves the plan, writes the file and stops before sending a single request: exit 0, nothing on stderr. `iyo conform dist --cases plan.json` exits 0 with no server running. Once `--origin` is given, the gate runs and the file is written alongside.

The written plan is a JSON object with keys `schema_version`, `cases`, `unresolved`.

Exit rule: `conform` has **no separate warning tier**. `error` and `warning` both gate on `conformant()`; only `never` turns the gate off.

`conformant()` is `failed == 0 && unresolved.is_empty() && !truncated`. An unresolved role or a deadline cut-off is a failure even when every case that ran passed.

An interrupt takes precedence and exits **130**.

A line goes to stderr before the first request, even under `-q`: `conform: 52 cases planned for <origin>`.

## `iyo serve`: flags

```
Usage: iyo serve [OPTIONS] [DIR]
```

**Positional:** `[DIR]`, *optional*, default `dist`. "The directory a build wrote."

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--port` | `-p` | `<PORT>` | 0–65535 | `8787` | Port to listen on. 0 asks the operating system for a free one |

Plus the six global flags. There is no `--base-url`, no `--config`: `iyo serve dist --base-url https://x/` → `error: unexpected argument '--base-url' found`.

Behaviour:

- Requires `<DIR>/manifest.json`. Missing → exit **3**: `<dir> has no manifest.json`, hint `run \`iyo build --out <dir>\` first`.
- `--json` writes **one line** to stdout, flushed immediately, then serves:
  ```json
  {"schema_version":"0.1","root":"/tmp/site/dist","address":"127.0.0.1:52713","url":"http://127.0.0.1:52713/","port":52713,"namespaces":3,"terms":10}
  ```
  With `--port 0` this is how a caller learns the bound port.
- Stderr always carries three lines: `serving <dir> on http://127.0.0.1:<port>/`, a namespace/term count line, and `for development only: one connection at a time, no TLS, bound to localhost.`
- A single SIGINT does **not** stop the server while it is blocked in `accept`. The first signal only sets the flag and restores the default disposition, so a second Ctrl-C is what ends it.

## `iyo diff`: flags

```
Usage: iyo diff [OPTIONS] <OLD> <NEW>
```

**Positionals:** both *required*.

| Positional | Required | Meaning (verbatim) |
|---|---|---|
| `<OLD>` | yes | The older release: a file, a directory or a glob |
| `<NEW>` | yes | The newer release. Add more with `--and` when a release is several files |

`iyo diff testdata/mini` (one argument) → `error: the following required arguments were not provided: <NEW>`, exit 2.

| Long | Short | Placeholder | Possible values | Default | Meaning (verbatim) |
|---|---|---|---|---|---|
| `--and` | — | `<INPUT>` | file/dir/pattern; **repeat the flag** | none | Further inputs for the newer release |
| `--and-old` | — | `<INPUT>` | file/dir/pattern; **repeat the flag** | none | Further inputs for the older release |
| `--exit-code` | — | none | — | off | Exit 1 when anything changed, for use in a pipeline |
| `--fail-on-breaking` | — | none | — | off | Exit 1 only when something breaking changed |

Plus the six global flags.

Exit rule: exits 1 when `(--fail-on-breaking && breaking > 0) || (--exit-code && anything changed)`, otherwise 0. Without either flag `diff` always exits 0.

`diff previous/ mini/ --exit-code` → 1. `diff mini/ mini/ --exit-code` → 0. `diff previous/ mini/ --fail-on-breaking` → 1 (that pair has `breaking: 2`).

Default (non-`--json`) output is **Markdown on stdout**, starting `# Changes in <title> <version>`. `-q` does not suppress it (35 stdout lines with and without `-q`).

## `iyo profiles`: flags

```
Usage: iyo profiles [OPTIONS]
```

**No positionals. No command-specific flags.** Only the six global flags.

Plain output, on stdout, one line per profile (`{:<10} {}`):
```
dcap       OWL application profile
dcmi       DCMI-style RDFS vocabulary
generic    Generic RDFS
shacl      SHACL shapes
skos       SKOS concept scheme
```

`--json` output is an **object**, not a bare array:
```json
{"schema_version":"0.1","profiles":[{"id":"dcap","title":"OWL application profile"}, …]}
```

`-q` does not suppress either form (5 stdout lines, 0 stderr lines).

## Positional arguments at a glance

| Command | Positional | Required? | Default when omitted | Repeatable |
|---|---|---|---|---|
| `check` | `<INPUT>...` | **yes** | — | yes |
| `model` | `<INPUT>...` | **yes** | — | yes |
| `build` | `[INPUT]...` | no | config file's `inputs`; error if neither | yes |
| `pdf` | `[DIR]` | no | `dist` | no |
| `probe` | `[TARGET]` | no | `dist` | no |
| `conform` | `[TARGET]` | no | `dist` | no |
| `serve` | `[DIR]` | no | `dist` | no |
| `diff` | `<OLD>` `<NEW>` | **yes**, both | — | no (use `--and` / `--and-old`) |
| `profiles` | — | — | — | — |

## Inputs: files, directories, patterns, and stdin (`-`)

All input resolution goes through one function, so `check`, `model`, `build` (`[INPUT]`, `--previous`) and `diff` (`<OLD>`, `<NEW>`, `--and`, `--and-old`) behave identically.

**Four accepted input shapes:**

| Shape | Behaviour |
|---|---|
| a file path | used directly |
| a directory | every RDF file **directly inside** it — **not recursive**. `iyo check testdata` (which contains only subdirectories) → exit 3, `no RDF files in …`, hint `iyo reads .ttl, .owl, .rdf, .nt and .jsonld; name a file directly if the extension is something else` |
| a pattern with `*` | the wildcard is matched against the **file name only**; the directory part must be literal. `'testdata/*/vocab.ttl'` fails (`reading directory …/testdata/*: No such file or directory`). `'testdata/**.ttl'` fails (`no files matched`) — there is no `**`. `'testdata/mini/*.ttl'` works. No match → exit 3, hint `quote the pattern so the shell does not expand it first, as in 'vocabularies/*.ttl'` |
| `-` | read RDF from stdin |

**Extensions recognised, and the parser each selects** (extension is lower-cased first, so `.TTL` and `.OWL` work):

| Extension | Parser |
|---|---|
| `.ttl` | turtle |
| `.nt`, `.txt` | ntriples |
| `.nq` | nquads |
| `.trig` | trig |
| `.n3` | n3 |
| `.owl`, `.rdf`, `.xml` | rdfxml |
| `.jsonld`, `.json` | jsonld |

Not recognised: `.turtle`, `.ntriples`, `.nquads`, `.rdfs`, `.md`, `.ttl.gz`.

**Stdin (`-`):**

- `-` means stdin, never a file literally named `-` in the working directory.
- It may appear **at most once**. Twice → exit **2**: `- given more than once; stdin can only be read to the end once`, hint `pass - at most once, with file paths for the other inputs`.
- It may be mixed with file paths: `cat vocab.ttl | iyo check - testdata/mini/scheme.ttl` read 2 files, 74 triples.
- Stdin has no extension, so the format is **sniffed from the first non-space byte**: `{` or `[` → JSON-LD; `<?xml`, `<rdf:RDF` or `<RDF` → RDF/XML; otherwise Turtle. N-Triples piped in is parsed by the Turtle parser (reported as `turtle`), which accepts it.
- Stdin is cited as `-` in every finding's `file` field, and appears as `"path": "-"` in the model's `files` array.
- `-` is **not** a stdin target for `probe` / `conform` / `serve` / `pdf`: `iyo probe -` treats `-` as a path and fails (`reading -: No such file or directory`).

## Exit codes

The constants, from the source:

| Code | Constant | Meaning (source doc comment) | Provoked by |
|---|---|---|---|
| 0 | `OK` | Success; no findings at error level. | `iyo check testdata/mini` (0 errors) |
| 1 | `FINDINGS` | Findings at error level, or warnings under `--strict`. | `iyo check bad.ttl` (2 errors); `iyo check testdata/mini --strict` (16 warnings); `iyo build bad.ttl` (refuses, writes nothing); `iyo build … --strict` with warnings; `iyo diff previous/ current/ --exit-code`; `iyo diff … --fail-on-breaking`; `iyo probe … --deadline 0` (truncated); `iyo conform` against a non-conformant origin |
| 2 | `USAGE` | Usage error. Emitted by clap. | `iyo buld`; `iyo check --nope`; `iyo check` (missing required `<INPUT>`); `iyo diff testdata/mini` (missing `<NEW>`); `iyo --color pink profiles`; `iyo` with no subcommand; `iyo check -V`. **Also produced by the tool itself** (not only clap): `--base-url "not a url"`; `--theme-switch --color-scheme dark`; `iyo check - -` |
| 3 | `INPUT` | Input could not be read or parsed. | `iyo check /nope.ttl`; `iyo check testdata` (no RDF files directly inside); `iyo check 'testdata/**.ttl'` (no match); `iyo build` with no inputs and no config; `iyo build --config missing.toml`; a config file with an unknown key; `iyo build --theme <not-a-dir>`; `iyo serve <dir>` with no `manifest.json`; `iyo pdf <dir>` with no `pdf/spec.typ`; `iyo probe http://unreachable/` (manifest fetch failed) |
| 4 | `ENVIRONMENT` | Environment error: a required external tool is missing. | `iyo pdf dist --typst-bin /nonexistent/typst`; `IYO_TYPST_BIN=/nonexistent/typst iyo pdf dist`; `iyo build … --pdf` when Typst is unavailable (site is still written) |
| 5 | `IO` | Output directory conflict or I/O failure. | `iyo build … --out <an existing regular file>` → `… exists and is a regular file; the build writes a directory there` |
| 130 | `INTERRUPTED` | Interrupted. The same number a shell synthesises for a process killed by SIGINT (128 + 2). | SIGINT to a running `iyo build` → `error: interrupted`, `hint: nothing was left half written; run the same command again when ready`, exit 130. `probe`/`conform` return 130 in preference to 1 when an interrupt is pending |

**141 is not one of these constants.** Writing to a closed pipe kills the process with SIGPIPE, and the shell then reports 128 + 13 = 141. It is a signal death rather than a value in `mod exit`. The run ends quietly at 141, the same way `yes | head` does. `iyo model big.ttl | head -c 10` → `PIPESTATUS[0]=141` (output must exceed the pipe buffer; a 25 KB model output fits in it and exits 0, a 4.4 MB one does not).

**One inconsistency:** `build --pdf` with Typst missing exits **4** *after* the site has been written successfully.

## `--json`: the contract, `schema_version`, and failing runs

`--json` sends machine-readable JSON to **stdout**, and the human summary that would otherwise go to stderr is suppressed. It is pretty-printed (2-space indent) everywhere except `serve`, which emits one compact line so a caller blocked on it gets the port immediately.

**`schema_version` is the string `"0.1"`** (`src/model.rs:13`), and it is the **first key** of every document that carries it.

| Command | `--json` document | Top-level keys |
|---|---|---|
| `check` | report object | `schema_version`, `findings`, `summary` (`summary` = `errors`, `warnings`, `info`, `by_rule`) |
| `model` | the model | `schema_version`, `generator`, `files`, `namespaces`, `documents`, `terms`, `prefixes`, `shapes`, `stats` — **printed with or without `--json`; the two are byte-identical** |
| `build` | build report | `schema_version`, `out_dir`, `files` (`path`, `bytes`), `digest`, `counts` (`documents`, `files`, `namespaces`, `terms`); plus `notes` **only when non-empty** |
| `pdf` (normal run) | **bare JSON array**, one object per PDF (`namespace`, `path`, `bytes`, `standards`, `typst_version`) — **no `schema_version` wrapper** |
| `pdf --dry-run` | object: `schema_version`, `dry_run`, `inputs` |
| `probe` | report object: `schema_version`, `origin`, `requests`, `made`, `truncated`, `findings`, `errors`, `warnings`, `foops`, `foops_status`; plus `responses` **only with `--full`** |
| `conform` | report object: `schema_version`, `origin`, `verdicts`, `unresolved`, `passed`, `failed`, `made`, `truncated` |
| `serve` | one line: `schema_version`, `root`, `address`, `url`, `port`, `namespaces`, `terms` |
| `diff` | `schema_version`, `old`, `new`, `counts`, `breaking`, `changes`; plus `scope` **only when the changelog covers one namespace** |
| `profiles` | `schema_version`, `profiles[]` (`id`, `title`) |
| `conform --cases <path>` (the written file) | `schema_version`, `cases`, `unresolved` |

`probe`'s `origin` is reported with any userinfo password masked (`user:***@host`).
`probe`'s `foops` / `foops_status` carry four probe ids: `CN1`, `RDF1`, `URI1`, `VER2`. `foops` is `true` / `false` / `null`. `foops_status` spells the same outcome over four states: `"pass"`, `"fail"`, `"not measured"`, `"not applicable"`.

**What a failing run emits.** An error that reaches the tool's own reporter writes this envelope to stdout, then the human message to stderr, then exits with the code named inside it:

```json
{
  "schema_version": "0.1",
  "error": {
    "message": "resolving inputs: no such file or directory: /tmp/site/nope.ttl",
    "hint": "check the path, or pass - to read RDF from stdin",
    "exit_code": 3
  }
}
```

`hint` is `null` when the error carries no suggested fix (e.g. `no inputs given and none configured`). The envelope is written even under `-q`.

**Three cases where `--json` produces no JSON on stdout:**

1. **clap usage errors** (exit 2 from argument parsing). `iyo check --json` with no input prints clap's usage to stderr and **nothing** to stdout: `err.exit()` runs before the reporter exists. Tool-raised exit-2 errors (`--base-url`, `--theme-switch`, `- -`) *do* get the envelope.
2. **`build` refusing on findings** (exit 1). `iyo build bad.ttl --json` writes the error-level findings and `refusing to build: run \`iyo check\` for the full report` to stderr and **nothing** to stdout.
3. **`build` refusing on its own HTML audit or theme contrast** (exit 1). Same shape: stderr only.

By contrast, `check --json` on a run with error findings **does** print the full report (exit 1 with a valid document on stdout).

## Streams: what goes to stdout, what goes to stderr

Primary output on stdout, messages on stderr. `-q` suppresses only the stderr summary and never touches stdout.

| Command | stdout | stderr | Does `-q` silence the stderr part? |
|---|---|---|---|
| `check` | findings table, or the `--json` report | 2-line summary: corpus stats, then `N errors, N warnings, N info` | yes (22 stdout / 0 stderr with `-q`) |
| `model` | the model JSON, always | — | n/a |
| `build` | nothing by default; the `--json` report when asked | `N files, N documents, N terms in N namespaces -> <out>`, `digest <sha256>`, `N pages audited, 0 errors, N warnings; theme contrast checked`, any `note:` lines, `dry run: nothing was written` | yes (`--json` alone also silences it) |
| `pdf` | nothing on a real run; the dry-run list, or the `--json` array | one line per compiled PDF | yes (0 / 0 with `-q`) |
| `probe` | `--json` report only | `probe: N requests planned for <origin>` (**not** silenced by `-q`), then the human summary (silenced by `-q`), progress line on a TTY only |  partly |
| `conform` | `--json` report only | `conform: N cases planned for <origin>` (**not** silenced by `-q`), then the human summary | partly |
| `serve` | the `--json` line when asked | 3 startup lines, then request log | yes for the startup lines |
| `diff` | Markdown changelog, or the `--json` report | — | no effect (35 / 0 either way) |
| `profiles` | the table, or the `--json` object | — | no effect (5 / 0 either way) |

On any failure, stderr always gets, in this order: `error: <message>`, `see: https://github.com/bffo-project/iyo`, and `hint: <what to do>` last, when the error carries one.

## Environment variables

All nine are clap `env` fallbacks on `build`/`pdf` flags, so a flag on the command line always wins.

| Variable | Fills | Commands | Effect |
|---|---|---|---|
| `IYO_OUT` | `--out` | `build` | `IYO_OUT=/tmp/site/envout iyo build …` wrote to that directory |
| `IYO_BASE_URL` | `--base-url` | `build` | the value appeared in the generated pages |
| `IYO_CONFIG` | `--config` | `build` | the named file was loaded in preference to `./iyo.toml` |
| `IYO_THEME` | `--theme` | `build` | the theme directory to use |
| `IYO_TYPST_BIN` | `--typst-bin` | `build`, `pdf` | `IYO_TYPST_BIN=/nonexistent/typst iyo pdf dist` → exit 4 |
| `IYO_COLOR_SCHEME` | `--color-scheme` | `build` | `IYO_COLOR_SCHEME=dark` produced the same digest as `--color-scheme dark` |
| `IYO_BASE_PATH` | `--base-path` | `build` | `href="/zzz/"` appeared in `404.html` |
| `IYO_LINK_STYLE` | `--link-style` | `build` | `IYO_LINK_STYLE=file` produced `href="…/Thing.html"` |
| `IYO_MD_FRONTMATTER` | `--md-frontmatter` | `build` | `IYO_MD_FRONTMATTER=jekyll` produced the `---` front-matter block |

Also read, but by the colour logic rather than by any flag: `NO_COLOR`, `TERM`, `CLICOLOR_FORCE`, `FORCE_COLOR` (see the colour section).

`curl` is invoked with `--netrc-optional`, so a credential for a protected origin can live in `~/.netrc` and the URL can be passed bare. Passing `https://user:pass@host` on the command line is accepted but warns:
```
warning: https://user:***@127.0.0.1:9 carries a credential in the command line, where argv, `ps` and the shell's history can all see it
hint: put the credential in ~/.netrc and pass the URL without it
```
The password is masked in the warning and in the JSON report's `origin`.

## Configuration file: resolution order and the accepted tables

**Only `iyo build` reads a configuration file.** No other subcommand has `--config`.

**Resolution order**, highest precedence first:

| # | Source | Effect |
|---|---|---|
| 1 | a command-line flag (e.g. `--base-url`) | `--base-url https://from-flag.example/` beat the file |
| 2 | `--config <FILE>` | `--config other.toml` beat `./iyo.toml`, and beat `IYO_CONFIG` |
| 3 | `IYO_CONFIG` (fills `--config`) | `IYO_CONFIG=env.toml` beat `./iyo.toml` |
| 4 | `./iyo.toml`, when it exists | picked up with no flag at all; also supplied `inputs` so `iyo build` ran with no positional argument |
| 5 | built-in defaults | used when none of the above is present |

A `--config` path that does not exist is an error, exit **3** (`reading <path>: No such file or directory`). It does not silently fall back to `./iyo.toml`.

**The exact set of accepted top-level keys is six**, and the parser refuses anything else rather than ignoring it. An unknown key names the accepted set in the error:

```
unknown field `nope`, expected one of `site`, `inputs`, `examples`, `narrative`, `namespaces`, `llms`
```

| Top-level key | Shape | Notes |
|---|---|---|
| `site` | table | see keys below |
| `inputs` | array of strings | files, directories or patterns; used when `iyo build` is given no positional |
| `examples` | string | path |
| `narrative` | string | path |
| `namespaces` | table of tables, keyed by namespace IRI | per-namespace overrides |
| `llms` | table | see keys below |

**`[site]` keys and their built-in defaults:**

| Key | Default |
|---|---|
| `base_url` | `"/"` (normalised to always end in `/`) |
| `lang` | `"en"` |
| `doc_license` | unset |
| `title` | unset |
| `cite_as` | `false` |
| `snapshots` | `"version-iri"` |
| `release` | unset |
| `latest` | `"release"` |
| `hosts` | `[]` |
| `pdf` | `false` |
| `previous` | `[]` |
| `md_frontmatter` | `"none"` |
| `link_style` | `"iri"` |
| `base_path` | unset |
| `external_paths` | `[]` |
| `theme_switch` | `false` |
| `color_scheme` | `"auto"` |

**`[namespaces."<iri>"]` keys:** `mount`, `resolver_prefix`, `url_style`, `stem`. (`url_style`: `flat` gives `/Format` with `Format.md` beside it; `dir` gives `/Format/` with `Format/index.md`.)

**`[llms]` keys:** `max_terms` (default `500`), `data_site` (unset).

Every table is `deny_unknown_fields`, so a misspelling in any of them is reported with the accepted list and exits 3. `[site] baseurl`, `[llms] maxterms` and `[namespaces.x] mountpoint` were each rejected this way. The hint reads: *the error above lists the keys this version accepts; iyo refuses keys it does not know rather than ignoring them, so a misspelling is reported instead of silently doing nothing*.

`external_paths` takes path prefixes on the same origin that belong to another application, so the build's link auditor does not report them as broken internal links.

## What makes a run exit 1 that is not `check`: the build's own gates

`iyo build` refuses to write (exit 1, stderr only, no JSON) in three situations.

**1. RDF findings.** `build` runs `check` with default options first. Any error-level finding stops it; with `--strict`, warnings do too. Message: the findings, then `refusing to build: run \`iyo check\` for the full report`.

**2. The build's own HTML audit.** Every rendered page is checked. An error-level issue stops the build; warnings are grouped by rule and printed as `warning  <rule>: N across the site, e.g. <page>: <message>`. The site is designed to meet WCAG 2.2 Level AA. What the audit actually checks is this fixed rule set:

| Rule | Level | Criterion recorded |
|---|---|---|
| `a11y.no-lang` | error | WCAG 3.1.1 |
| `a11y.lang-invalid` | error | WCAG 3.1.1 |
| `a11y.lang-part-invalid` | error | WCAG 3.1.2 |
| `a11y.no-html-element` | error | parsing |
| `a11y.charset-late` | error | HTML validity |
| `a11y.no-viewport` | error | WCAG 1.4.10 |
| `a11y.no-title` | error | WCAG 2.4.2 |
| `a11y.missing-landmark` | error | axe region, WCAG 1.3.1 |
| `a11y.multiple-main` | error | axe landmark-one-main |
| `a11y.unnamed-nav` | error | axe landmark-unique |
| `a11y.no-h1` | error | WCAG 1.3.1 |
| `a11y.multiple-h1` | warning | axe page-has-heading-one |
| `a11y.heading-skip` | error | axe heading-order, WCAG 1.3.1 |
| `a11y.empty-id` | error | HTML validity |
| `a11y.invalid-id` | error | HTML validity, html-validate valid-id |
| `a11y.duplicate-id` | error | axe duplicate-id, WCAG 4.1.1 |
| `a11y.dangling-fragment` | error | WCAG 2.4.4 |
| `a11y.empty-link` | error | axe link-name, WCAG 2.4.4 |
| `a11y.broken-internal-link` | warning | link integrity |
| `a11y.table-no-caption` | warning | WCAG 1.3.1, technique H39 |
| `a11y.table-no-headers` | error | axe td-has-header, WCAG 1.3.1 |
| `a11y.th-no-scope` | warning | WCAG 1.3.1, technique H63 |
| `html.absolute-nav-link` | error | link integrity |
| `html.escaped-markup` | error | template correctness |
| `markdown.absolute-link` | — | link integrity |
| `markdown.broken-link` | — | link integrity |

**3. Theme contrast.** Every declared colour pair is measured. Thresholds: body text 4.5:1; large text, non-text and link-vs-text 3.0:1 (rule ids `theme.contrast-text`, `theme.contrast-large-text`, `theme.contrast-non-text`, `theme.contrast-link-vs-text`). A failing pair stops the build:
```
  <scheme> <fg> on <bg> is <ratio>:1, needs <required>:1  (<site>)
      <fg value> on <bg value>
```
Up to 20 issues and 20 failing pairs are listed, then `... and N more`.

The final line names what actually stopped the build: `refusing to write: N accessibility errors in the rendered pages and N failing colour pairs` (only the causes that fired).

## Runtime requirements beyond the binary

| Tool | Needed by | Missing behaviour |
|---|---|---|
| `curl` | `probe`, `conform` | Checked before any request is planned. Error: `curl was not found on PATH`, **exit 4**. The tool's `-w` format uses `%header{}`, which needs curl 7.84 or newer. |
| `typst` | `pdf`, and `build --pdf` | `pdf` exits **4** with `… is not on PATH. Install Typst (https://typst.app/open-source/) or pass --typst-bin`. `build --pdf` writes the site first, then exits 4 with `the site was written; the PDF was not: …` and a hint to run `iyo pdf <out>` later. |

Neither is needed for `check`, `model`, `build` (without `--pdf`), `diff`, `serve` or `profiles`.

