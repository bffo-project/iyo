# Checking negotiation, four ways

`conformance.json` is the negotiation contract of `docs/output-convention.md`,
written once as role-based cases and resolved against a manifest by one
resolver (`src/conform/resolve.rs`). Four implementations answer to it:

| Implementation | Checked by | What it adds |
| --- | --- | --- |
| Rust resolver | `tests/negotiate.rs`, on every `cargo test` | the decision itself |
| Generated Cloudflare Worker | `worker-negotiation.mjs` | the JavaScript that will actually run in production |
| `iyo serve` | `serve-negotiation.py` | the HTTP layer around the decision: the request line, header parsing, the status line, `Location` |
| A live origin | `iyo conform <target> --origin URL`, no local harness needed | the only one that reads the body, and the only one measuring something this tool did not just build |

They are measured against the file rather than against each other, so two
implementations agreeing on a wrong answer still fails. Adding a case to
`conformance.json` adds it to all four at once, which is the point of writing
it down separately from any of them.

`apache-rules.py` is a fifth, different kind of check, unaffected by this
file: the structure of what the adapters emit is checked in `tests/adapter.rs`,
but a configuration file can be perfectly well-formed and still send every
request to the wrong place. It derives its own cases from `manifest.json`
directly (see its own section below) and does not read `conformance.json` at
all.

## Cases name a role, not a path

`negotiation-matrix.json` used to name literal paths -- `/vocab/Widget`,
`/vocab/0.1.0/Widget` -- which exist on `testdata/mini` and nowhere else, so
the file could not be run against a real deployment. `conformance.json`
instead names a **subject role**, resolved against whatever manifest is at
hand:

```json
{ "name": "turtle is redirected to its sibling", "group": "negotiation",
  "subject": "term", "accept": "text/turtle",
  "expect": { "kind": "redirect", "media_type": "text/turtle" } }
```

Running the contract against `testdata/mini` and against a production release
is the same operation with a different manifest. The full list of roles --
`term`, `dir-term`, `sibling`, `namespace`, `nested-namespace`,
`release-term`, `empty-namespace`, `reserved-segment`, `sub-term-path`,
`absent-name`, `absent-release`, `case-variant`, `sibling-unpublished`,
`absent-release-name` -- and how each is resolved is specified in
one contract; the resolver they all go through is
`src/conform/resolve.rs`, so there is exactly one implementation of what "a
dir-layout term" means, the same reason `src/adapter/*` has exactly one
implementation of what a manifest says a host should emit.

A role that finds no subject on a given manifest fails the run: `resolve()`'s
`unresolved` list exists precisely so a role that never got the chance to run
cannot hide inside a passing summary. An earlier version of this file let a
case opt out of that with `"optional": true`; the one case that used it
(`the deepest mount wins`) turned out to be a byte-for-byte duplicate of
another case in the same file, so its `optional` flag could never fire and
`skipped_optional` was permanently empty everywhere it was threaded. The
mechanism was removed along with the duplicate rather than left in place
describing a check nothing exercises -- the substance is not lost:
`resolve()` already iterates every namespace, so `term` on
`/vocabulary/category/` produces a path that also lies under the shallower
`/vocabulary/` mount, and a host resolving against the wrong one still
produces a different `Location`.

## Building and resolving

All four implementations need a built site and a resolved copy of the
contract against it. From the repository root:

```sh
cargo run -q -- build testdata/mini --out /tmp/iyo-site \
  --base-url https://example.org/ --host cloudflare --host apache
cargo run -q -- conform /tmp/iyo-site --cases /tmp/cases.json
```

The second command, given a local target and no `--origin`, resolves the
contract against `/tmp/iyo-site/manifest.json` and writes the plan to
`/tmp/cases.json` **without making any HTTP request** -- it exits 0 whether or
not anything is listening anywhere. That is what lets the two local harnesses
below run against nothing but a directory: they read `/tmp/cases.json`
(overridable with the `IYO_CASES` environment variable, or a second argument
to `serve-negotiation.py`) rather than re-resolving the contract themselves.

The base URL has to be the one the fixture's own IRIs use. A snapshot is
planned only where `owl:versionIRI` points at the URL the snapshot would be
published at, so building the fixture at a different origin silently drops
the release and every `release-term` case becomes unresolvable. The paths
under test are origin-relative, so serving the built tree on a local port
still works.

The plan is a `--json` document like any other: `schema_version` first, then
`cases` and `unresolved`.

`conformance.json` currently holds 29 written cases, resolving to 52 concrete
requests against `testdata/mini` -- most roles resolve once per namespace, so
the count is not fixed and neither harness nor this document hardcodes it.
Read `cases.length` from the plan rather than assuming a number.

## What `expect.kind` means, and who can tell

| Kind | Means | Checked as |
| --- | --- | --- |
| `serve` | this IRI answers here | 200, the namespace's default media type, and the body contains the subject's identity IRI |
| `redirect` | negotiated to a sibling | the manifest's own status code, and `Location` equal to the resolved sibling |
| `file` | asked for by name; not a negotiation | 200 and that exact media type |
| `absent` | nothing is minted here | 404, and explicitly not 200 |

`file` and `absent` are opposite results. The matrix this replaced conflated
them as a single `pass` outcome that accepted "200 or 404" -- exactly what
would let an application's catch-all route through undetected. Nothing below
does that: every harness gives `file` and `absent` distinct checks.

A `serve` case's `as` has to be the namespace's own `default_type`. The file
and the `Link` header a `serve` case resolves to are the default
representation's either way, so a case naming another type would be judged
against a type that file never carries -- unsatisfiable by a correct host.
`resolve` refuses such a case into `unresolved` rather than resolving it, and
an unresolved case fails the run. A representation at a *different* address
is a `file` case; one negotiated *at this address* is a `redirect` case.

`expected_link`, computed by the resolver for every `serve` and `redirect`
case, is the exact `Link` header the manifest says the host should emit (or
`null` for `file`/`absent`, which are not negotiations). Where it is
non-null, both local harnesses compare it byte for byte -- this is the check
that caught a release pointing `describedby` at its parent's `llms.txt`
instead of its own (commit `125bf97`); knowing the relations were merely
*present* said nothing about where they pointed.

### What each harness cannot distinguish

| Harness | `serve` | `redirect` | `file` | `absent` |
| --- | --- | --- | --- | --- |
| Rust resolver (`tests/negotiate.rs`) | file path | yes | **indistinguishable** -- no filesystem | **indistinguishable**, same reason |
| Generated Worker (`worker-negotiation.mjs`) | yes, incl. body | yes | yes, once the asset stub knows the built tree | yes, same |
| `iyo serve` (`serve-negotiation.py`) | yes, incl. body | yes | yes | yes |
| Live origin (`iyo conform`) | yes, incl. body | yes | yes | yes |

`negotiate::resolve` returns the same `PassThrough` outcome for both `file`
and `absent`, because deciding between them needs a filesystem the resolver
does not have. `tests/negotiate.rs` counts these cases as skipped rather than
silently asserting nothing about them, and prints how many.

The other three harnesses have a real filesystem or a real socket behind
them and skip nothing: each prints `0 skipped as indistinguishable` so that
is a statement, not an absence of one. The convention's three normative headers
(`Vary: Accept`, an explicit `Cache-Control`, open CORS) are checked on every
kind except `absent` -- `serve`, `redirect`, *and* `file` -- by every harness
capable of reading response headers, which is all but the Rust resolver.
This is wider than "every negotiated response" reads at first: the
output convention states the cache values as a rule about *paths*, not about
negotiated responses specifically ("Snapshot paths carry
`Cache-Control: public, max-age=31536000, immutable`; latest carries
`max-age=300, must-revalidate`"), and `NamespaceEntry::cache_control`'s own
doc comment reads "what a host should send for this namespace's own files" --
files, not only negotiated term pages. A sibling asked for by name and a
namespace's own document are both that namespace's own files, so `file` gets
the same header check as `serve` and `redirect`; only `absent` (a 404) is
exempt, because nothing there is this namespace's file. `src/conform/judge.rs`
enforces exactly this scope, and both local harnesses now match it -- an
earlier version of this migration scoped the check to `serve`/`redirect`
only, which let the generated Worker under-serve headers on every `file`-kind
response without either local harness noticing.

## `worker-negotiation.mjs`

Runs the generated Cloudflare Worker in Node with the asset layer stubbed by
the actual built tree, so it can answer 404 for a path the build did not
write and 200 with the real bytes for one it did -- and a `serve` case's body
check has something real to look for.

```sh
cp /tmp/iyo-site/adapters/cloudflare/worker.js tests/hosts/worker.mjs
IYO_SITE=/tmp/iyo-site node tests/hosts/worker-negotiation.mjs
```

`IYO_CASES` overrides where it reads the resolved plan from (default
`/tmp/cases.json`).

## `serve-negotiation.py`

Drives a running `iyo serve` over HTTP, which is the only one of the local
harnesses that exercises the request parsing, the status line and the
`Location` header as bytes on a socket rather than as a return value.

```sh
cargo run -q -- serve /tmp/iyo-site --port 8788 &
python3 tests/hosts/serve-negotiation.py http://127.0.0.1:8788
```

Takes the resolved-cases path as an optional second argument, or `IYO_CASES`,
default `/tmp/cases.json`. It also asserts one thing outside the contract
proper: a request for `/../Cargo.toml` still 404s, because that is a real
property of the HTTP layer this harness is the only one of the three to have
a socket for.

`iyo serve` now satisfies the contract completely: `iyo conform` against it
passes all resolved cases (52 of 52 on `testdata/mini`). That was not true
earlier in this project's history, which is why
this harness can assert the full contract rather than a subset.

## A live origin, via `iyo conform`

The fourth implementation needs no file in this directory at all:

```sh
cargo run -q -- conform /tmp/iyo-site --origin http://127.0.0.1:8788
# or, against a deployment:
cargo run -q -- conform https://bffo.org
```

`conform` resolves the bundled contract itself and reports pass/fail grouped
by rule. It is the only implementation
that reads the response body for every `serve` case rather than trusting the
status line -- the check that tells a real term page apart from an
application's catch-all route answering 200 for everything.

## `apache-rules.py`

Applies the generated `.htaccess` to a matrix of requests in Apache's order,
interpreting the subset the adapter emits. It answers two questions: does
every term IRI resolve to the right file, and does any rule redirect a
request to itself. This file is unchanged by the migration above; it already
derived its cases from `manifest.json` rather than from a written contract.

```sh
python3 tests/hosts/apache-rules.py /tmp/iyo-site
```

It derives its cases from `manifest.json`, so it runs on whatever was built:
75 checks on `testdata/mini`, hundreds on a real release. An earlier version
had a real release's paths written into it and reported failures on the
fixture its own instructions told you to build.

The second question is why this exists. The first version of the adapter
redirected a term to its own page, which is an infinite redirect, and did the
same for a nested namespace document. Both files were well-formed and both
would have taken a site down.

## Proving a harness still fails on a real defect

A harness that reads a contract but does not actually compare against it is
worse than none: it reports success on the strength of a file it never
really checked. Before trusting any change to these three files, perturb one
expectation in `conformance.json` -- flip a `media_type`, change a
`location` -- rebuild the resolved cases, and confirm the Rust test, the
Worker harness and the `iyo serve` harness each report a failure; `iyo
conform` run directly against a live `iyo serve` should too. Then restore it.
`apache-rules.py` is unaffected by design, since it does not read this file.
This is the same check that once caught the signposting additions being
inert: a green summary line is not evidence the check underneath it does
anything.
