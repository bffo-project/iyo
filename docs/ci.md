# Running iyo in CI

[README](../README.md) · [cli](cli.md) · [rules](rules.md) · [theming](theming.md) · [output-convention](output-convention.md) · **ci**

How to use iyo as a gate in a pipeline that publishes a vocabulary. Every command
returns an exit code a job can branch on, and every command takes `--json`.

For the meaning of each flag, see [cli.md](cli.md). This page is about wiring
them together.

## Exit codes, from a pipeline's point of view

| Code | What it means for the job |
| --- | --- |
| 0 | The gate passed |
| 1 | The gate failed: findings at error level, a breaking change, a non-conformant origin |
| 2 | The job is wrong: a bad flag or a missing argument |
| 3 | The inputs are wrong: a path that does not exist, a config key that is not recognised |
| 4 | The runner is missing `curl` or `typst` |
| 5 | The output path could not be written |
| 130 | Interrupted |

Codes 2 to 5 mean the pipeline needs fixing, not the vocabulary. Treating
anything non-zero as "the vocabulary is broken" will mislead whoever reads the
failure.

## Lint on every pull request

```bash
iyo check ontology/*.ttl --strict
```

`check` writes nothing and makes no network requests. Without `--strict` only
errors fail the job; with it, warnings do too. Start without it, see what the
vocabulary reports, then turn it on once the backlog is clear.

To fail on some rules and not others, filter by id:

```bash
iyo check ontology/*.ttl --strict --ignore term.no-is-defined-by
```

`--ignore` can suppress error-level rules and turn a failing job green, so the
run prints how many findings it hid. A filter that matches no rule is a usage
error, exit 2, rather than a job that checks nothing.

## Build as a gate

```bash
iyo build ontology/*.ttl --out dist --base-url https://example.org/ --strict
```

The build refuses to write when a page audit finds an error, when a colour pair
fails the contrast gate, or when a theme token cannot be read. `--strict` adds
the RDF lint's warnings to that list.

A refused build leaves nothing behind, so a job that fails here has not
half-published anything. The same holds for an interrupted one.

## Block a breaking change

```bash
iyo diff previous/ ontology/ --fail-on-breaking
```

Exits 1 when a term was removed, renamed, or had its meaning narrowed. Use
`--exit-code` instead to fail on any change at all, which suits a repository
where the vocabulary is meant to be stable between releases.

`--json` carries a `breaking` count and a `changes` array, so a job can post the
list on the pull request rather than making a reviewer read the log.

## Gate the deployment

After the site is live, check that the host actually performs the negotiation
the manifest describes:

```bash
iyo conform dist --origin https://staging.example.org
```

This is the check worth running on every deploy. It sends a fixed set of cases,
one subject per namespace per rule, and finishes in seconds. Against a host that
negotiates correctly it passes every case; against a plain file server it fails
most of them, because a file server cannot vary on `Accept`.

**A case that could not be resolved counts as a failure.** The summary line
reports passed and failed, and unresolved cases are listed above it. A run with
`0 failed` can still exit 1 for this reason, which is deliberate: a case the
gate could not run is not a case that passed.

`iyo probe` is the exhaustive version. It requests every IRI the manifest mints,
takes minutes rather than seconds, and suits a nightly job rather than a deploy
gate.

## What the runner needs

- `curl` on `PATH`, for `probe` and `conform`. Without it they exit 4.
- `typst`, only for `pdf` and `build --pdf`. Without it the site is still
  written and the PDF is not.
- No network access for `check`, `build`, `model` or `diff`. They read files and
  write files.

Two builds of the same inputs produce identical output, so a digest from a
previous run is a usable cache key.

## A complete workflow

Pin the version. A gate should change when you decide it changes, not when iyo
releases. `cargo install` compiles from source, so cache `~/.cargo/bin` if the
extra minute matters.

```yaml
name: vocabulary

on:
  pull_request:
  push:
    branches: [main]

jobs:
  vocabulary:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install iyo
        run: cargo install --locked iyo@0.1.0

      - name: Lint
        run: iyo check ontology/*.ttl --strict

      - name: Build
        run: iyo build ontology/*.ttl --out dist --base-url https://example.org/ --strict

      - name: Breaking changes
        if: github.event_name == 'pull_request'
        run: iyo diff previous/ ontology/ --fail-on-breaking
```

Add the deployment gate as a separate job, after the site is live:

```yaml
      - name: Gate the deployment
        run: iyo conform dist --origin https://example.org --json > conform.json
```

Keep `conform.json`. It records which cases ran, which passed, and what the
origin returned, which is the difference between "the deploy broke negotiation"
and "the deploy was fine and the gate is misconfigured".
