//! The command line.
//!
//! Conventions follow clig.dev: primary output
//! on stdout and messages on stderr, `--json` as the stable interface, no
//! prompts ever, documented exit codes, and `NO_COLOR` respected.

use crate::{
    build, check, config, conform, diff, exit, http, interrupt, load, model, pdf, probe, profile,
    render, serve, site,
};
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

/// Where every help screen points a reader who wants more than the flag
/// table. A macro rather than a `const` so `concat!` can take it, which is
/// what keeps every screen on one spelling of the URL.
macro_rules! docs_url {
    () => {
        "https://github.com/bffo-project/iyo"
    };
}

/// The last line of every long help screen.
const DOCS_TRAILER: &str = concat!("Documentation: ", docs_url!());

/// Per-command examples. clig.dev G5 asks for them in the help itself, where
/// a reader already is, rather than only in a README they have to find, and
/// *before* the flag table: a reader who wants the examples should not have
/// to scroll a screen of options to reach them. Each block is spliced into
/// its command's `long_about`, which clap prints between the description and
/// `Usage:`; the documentation URL is the `after_long_help` trailer below,
/// so no help screen is a dead end either way.
macro_rules! examples {
    ($($name:ident = $body:expr;)*) => {
        $(
            macro_rules! $name {
                () => {
                    concat!("Examples:\n", $body)
                };
            }
        )*
    };
}

examples! {
    CHECK_EXAMPLES = "  # Every finding, as a table on stdout
  iyo check ontology/ 'vocabularies/*.ttl'

  # Fail the build on warnings too, and report as JSON for CI
  iyo check ontology/ --strict --json

  # Narrow to one rule family while fixing it
  iyo check ontology/ --select term.no-label,term.no-definition";

    MODEL_EXAMPLES = "  # The model a theme or a plugin sees, pretty-printed
  iyo model ontology/bffo.ttl 'vocabularies/*.ttl'

  # Each namespace and how many terms it mints, writing nothing
  iyo model ontology/ --json | jq -r '.namespaces[] | \"\\(.iri) \\(.term_count)\"'";

    BUILD_EXAMPLES = "  # A site for a real origin, with the host's rules compiled beside it
  iyo build ontology/ 'vocabularies/*.ttl' --out dist \\
      --base-url https://example.org/ --host cloudflare

  # What would be written, and the digest it would have, writing nothing
  iyo build ontology/ --dry-run --json

  # Markdown a Hugo site can consume directly
  iyo build ontology/ --out dist --md-frontmatter hugo";

    PDF_EXAMPLES = "  # Compile the PDF inputs `build --pdf` prepared
  iyo pdf dist

  # With a Typst that is not on PATH
  iyo pdf dist --typst-bin ~/.local/bin/typst";

    PROBE_EXAMPLES = "  # Every IRI in a built manifest, against the origin it was built for
  iyo probe dist/manifest.json

  # A staging origin, using the manifest the build made for production
  iyo probe dist --origin https://staging.example.org --deadline 120

  # Ten terms per namespace is usually enough to find a broken host
  iyo probe dist --sample 10 --json";

    CONFORM_EXAMPLES = "  # Does this origin implement the negotiation convention?
  iyo conform dist --origin https://example.org

  # The plan the gate would run, without making a single request
  iyo conform dist --cases plan.json

  # Gate a pull request: any failure or unresolved case exits 1
  iyo conform dist --origin https://staging.example.org --json";

    SERVE_EXAMPLES = "  # Serve a built site with its own negotiation in front of it
  iyo serve dist --port 8787

  # Let the OS pick the port, and read it back from stdout
  iyo serve dist --port 0 --json";

    DIFF_EXAMPLES = "  # What changed between two releases
  iyo diff previous/ current/

  # Fail a release when anything breaking changed
  iyo diff previous/ current/ --exit-code --json";

    PROFILES_EXAMPLES = "  # The profiles this build knows about
  iyo profiles

  # Just the names, for a shell completion or a script
  iyo profiles --json | jq -r '.profiles[].id'";
}

/// `-h` stays one screen: clig.dev G4 asks for concise help without
/// arguments and on `-h`, and the full treatment only on `--help`. This is
/// also what a reader gets when they run `iyo` with no subcommand at all.
const SHORT_HELP: &str = "\
Run `iyo <command> --help` for a command's flags and examples, or
`iyo --help` for examples of the whole pipeline.";

macro_rules! EXAMPLES {
    () => {
        "\
Examples:
  # Lint a multi-file release and print findings as JSON
  iyo check ontology/ vocabularies/ --json

  # Build a site for a real origin and compile the host's rules with it
  iyo build ontology/ 'vocabularies/*.ttl' --out dist \\
      --base-url https://example.org/ --host cloudflare

  # Serve what was built, with the manifest's negotiation in front of it
  iyo serve dist --port 8787

  # Gate a deployment: does this origin implement the convention?
  iyo conform dist --origin https://example.org

  # Inspect the intermediate model that renderers and themes see
  iyo model ontology/bffo.ttl 'vocabularies/*.ttl'
"
    };
}

#[derive(Parser, Debug)]
#[command(
    name = "iyo",
    version,
    about = "Publish OWL, RDFS, SKOS and SHACL vocabularies as a static site.",
    long_about = concat!(
        "Publish OWL, RDFS, SKOS and SHACL vocabularies as a static site: \
per-term HTML, Markdown, Turtle and JSON-LD, an llms.txt agent index, and a \
host-neutral manifest that compiles into one host's negotiation rules.\n\n",
        EXAMPLES!()
    ),
    after_help = SHORT_HELP,
    after_long_help = DOCS_TRAILER
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress the summary on stderr.
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Never colour the output. The same as `--color never`.
    #[arg(long, global = true)]
    no_color: bool,

    /// When to colour messages: auto (a terminal), always, or never.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    color: ColorWhen,

    /// Explain each step on stderr as it happens.
    #[arg(short = 'd', long = "debug", visible_alias = "verbose", global = true)]
    debug: bool,
}

/// `--color`'s three values, as clig.dev G8 and every other CLI spell them.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

// `Build` carries every build flag and is much larger than the other eight
// variants. Boxing it is the usual fix and is not available here: clap's
// derive reads the fields off the variant. The cost this lint exists to
// prevent is paid per value, and exactly one `Command` is constructed per
// process, before any work starts.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum Command {
    /// Lint the RDF of a release without writing anything.
    ///
    /// Reads files, not a deployment: `probe` and `conform` are the two
    /// that talk to a live origin.
    #[command(long_about = concat!("Lint the RDF of a release without writing anything.\n\nReads files, not a deployment: `probe` and `conform` are the two that\ntalk to a live origin.", "\n\n", CHECK_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Check {
        /// Files, directories or patterns such as 'vocabularies/*.ttl'.
        #[arg(value_name = "INPUT", required = true)]
        inputs: Vec<String>,

        /// Exit non-zero on warnings as well as errors.
        #[arg(long)]
        strict: bool,

        /// Report literals with no language tag.
        #[arg(long)]
        strict_lang: bool,

        /// Only run rules whose id starts with one of these.
        #[arg(long, value_delimiter = ',', value_name = "RULE")]
        select: Vec<String>,

        /// Skip rules whose id starts with one of these.
        #[arg(long, value_delimiter = ',', value_name = "RULE")]
        ignore: Vec<String>,
    },

    /// Print the intermediate model as JSON.
    ///
    /// This is the contract that renderers, themes and any future plugin read.
    #[command(long_about = concat!("Print the intermediate model as JSON.\n\nThis is the contract that renderers, themes and any future plugin read.", "\n\n", MODEL_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Model {
        #[arg(value_name = "INPUT", required = true)]
        inputs: Vec<String>,
    },

    /// Render the site into an output directory.
    #[command(long_about = concat!("Render the site into an output directory.", "\n\n", BUILD_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Build {
        #[arg(value_name = "INPUT")]
        inputs: Vec<String>,

        /// Where to write. Created if missing.
        #[arg(
            short,
            long,
            visible_alias = "output",
            value_name = "DIR",
            default_value = "dist",
            env = "IYO_OUT"
        )]
        out: Utf8PathBuf,

        /// Absolute URL of the site root. Defaults to the origin of the
        /// release's root document.
        #[arg(long, value_name = "URL", env = "IYO_BASE_URL")]
        base_url: Option<String>,

        /// Configuration file. Defaults to ./iyo.toml when it exists.
        #[arg(long, value_name = "FILE", env = "IYO_CONFIG")]
        config: Option<Utf8PathBuf>,

        /// Refuse to write when there are warnings as well as errors.
        #[arg(long)]
        strict: bool,

        /// A theme directory whose files override the built-in theme one by
        /// one.
        #[arg(long, value_name = "DIR", env = "IYO_THEME")]
        theme: Option<Utf8PathBuf>,

        /// When to write a versioned snapshot of a namespace: only where a
        /// version IRI points at one, for every versioned namespace, or never.
        #[arg(long, value_name = "WHEN", value_parser = ["version-iri", "all", "none"])]
        snapshots: Option<String>,

        /// Version string to use when the RDF declares none.
        #[arg(long, value_name = "VERSION")]
        release: Option<String>,

        /// The previous release, to write a changelog against. Repeatable,
        /// because a release is usually several files.
        #[arg(long, value_name = "INPUT")]
        previous: Vec<String>,

        /// Write the Typst template and data for a PDF of each namespace,
        /// and compile them when `typst` is available.
        #[arg(long)]
        pdf: bool,

        /// Where `typst` is, when it is not on PATH.
        #[arg(
            long,
            value_name = "PATH",
            default_value = "typst",
            env = "IYO_TYPST_BIN"
        )]
        typst_bin: String,

        /// Compile the manifest into a host's configuration. Repeatable.
        #[arg(long = "host", value_name = "HOST",
              value_parser = ["cloudflare", "apache", "vercel", "dcmi-ns", "github-pages"])]
        hosts: Vec<String>,

        /// Which colour schemes to publish: `auto` (both, the reader's system
        /// chooses), `light` or `dark`. Forcing one is for a site that has to
        /// match surroundings offering only that one.
        #[arg(long, value_name = "SCHEME", value_parser = ["auto", "light", "dark"], env = "IYO_COLOR_SCHEME")]
        color_scheme: Option<String>,

        /// Offer readers a light/dark control. This is the only JavaScript
        /// this tool emits: without it a page follows the operating system's
        /// setting, which is what it does today.
        #[arg(long)]
        theme_switch: bool,

        /// The path the site is served from, when that differs from the path
        /// in `--base-url`. Only the 404 page needs it, because it is the one
        /// page whose links cannot be relative.
        #[arg(long, value_name = "PATH", env = "IYO_BASE_PATH")]
        base_path: Option<String>,

        /// Where navigation links point: `iri` (the term IRI, the default and
        /// correct wherever negotiation is implemented) or `file` (the
        /// document that exists, for a host that serves files and nothing
        /// else). Identity is unaffected either way.
        #[arg(long, value_name = "STYLE", value_parser = ["iri", "file"], env = "IYO_LINK_STYLE")]
        link_style: Option<String>,

        /// Prefix per-term Markdown with YAML front matter for another site
        /// generator to consume the tree.
        #[arg(
            long,
            value_name = "STYLE",
            value_parser = ["none", "hugo", "mkdocs", "jekyll"],
            env = "IYO_MD_FRONTMATTER"
        )]
        md_frontmatter: Option<String>,

        /// Report what would be written without writing it.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },

    /// Compile the PDF of each namespace from a built directory.
    #[command(long_about = concat!("Compile the PDF of each namespace from a built directory.", "\n\n", PDF_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Pdf {
        /// The directory a build wrote, with `--pdf`.
        #[arg(value_name = "DIR", default_value = "dist")]
        dir: Utf8PathBuf,

        /// PDF standards to enforce. Defaults to what the installed Typst
        /// supports: `ua-1,a-2a` from 0.15, `a-2a` before it.
        #[arg(long, value_name = "LIST")]
        pdf_standard: Option<String>,

        /// Where `typst` is, when it is not on PATH.
        #[arg(
            long,
            value_name = "PATH",
            default_value = "typst",
            env = "IYO_TYPST_BIN"
        )]
        typst_bin: String,

        /// A directory of fonts to use instead of Typst's embedded ones.
        #[arg(long, value_name = "DIR")]
        font_path: Vec<Utf8PathBuf>,

        /// List what would be compiled without running Typst. `pdf` writes
        /// files like `build` does, so it takes the same flag.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },

    /// Probe every IRI on a live origin: exhaustive, minutes.
    ///
    /// Asks the manifest for every IRI it mints and every representation
    /// each one advertises, then requests all of them. `conform` is the
    /// short version: a fixed set of cases that answers whether the host
    /// implements the convention at all.
    #[command(long_about = concat!("Probe every IRI on a live origin: exhaustive, minutes.\n\nAsks the manifest for every IRI it mints and every representation each\none advertises, then requests all of them. `conform` is the short\nversion: a fixed set of cases that answers whether the host implements\nthe convention at all.", "\n\n", PROBE_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Probe {
        /// A built directory, a manifest file, or the URL of a deployed site.
        #[arg(value_name = "TARGET", default_value = "dist")]
        target: String,

        /// The origin to send requests to. Defaults to the manifest's own
        /// site root, or to TARGET when it is a URL.
        #[arg(long, value_name = "URL")]
        origin: Option<String>,

        /// Probe at most this many terms per namespace.
        #[arg(long, value_name = "N")]
        sample: Option<usize>,

        /// Seconds to wait for one request.
        #[arg(long, default_value_t = 15)]
        timeout: u32,

        /// Stop the whole run after this many seconds and report what was
        /// gathered, rather than continuing against a host that may never
        /// answer.
        #[arg(long, value_name = "SECONDS", default_value_t = 300)]
        deadline: u64,

        /// Include every response in the JSON output.
        #[arg(long)]
        full: bool,

        /// What counts as failure.
        #[arg(long, value_name = "LEVEL", default_value = "error",
              value_parser = ["error", "warning", "never"])]
        fail_on: String,
    },

    /// Gate a live origin on the negotiation convention: fixed cases, seconds.
    ///
    /// A gate, not a survey: one subject per namespace per rule, so a
    /// deployment can be checked on every push. `probe` is the one that
    /// asks whether every IRI resolves.
    #[command(long_about = concat!("Gate a live origin on the negotiation convention: fixed cases, seconds.\n\nA gate, not a survey: one subject per namespace per rule, so a deployment\ncan be checked on every push. `probe` is the one that asks whether every\nIRI resolves.", "\n\n", CONFORM_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Conform {
        /// A built directory, a manifest file, or the URL of a deployed site.
        #[arg(value_name = "TARGET", default_value = "dist")]
        target: String,

        /// The origin to send requests to. Defaults to the manifest's own
        /// site root, or to TARGET when it is a URL.
        #[arg(long, value_name = "URL")]
        origin: Option<String>,

        /// Write the resolved cases here, for the host harnesses to run.
        #[arg(long, value_name = "PATH")]
        cases: Option<Utf8PathBuf>,

        /// Seconds to wait for one request.
        #[arg(long, default_value_t = 15)]
        timeout: u32,

        /// Stop the whole run after this many seconds and report what was
        /// gathered, rather than continuing against a host that may never
        /// answer. `conform` is meant to finish in seconds, so its default
        /// is far tighter than `probe`'s.
        #[arg(long, value_name = "SECONDS", default_value_t = 60)]
        deadline: u64,

        /// What counts as failure.
        #[arg(long, value_name = "LEVEL", default_value = "error",
              value_parser = ["error", "warning", "never"])]
        fail_on: String,
    },

    /// Serve a built site, honouring the manifest's negotiation.
    #[command(long_about = concat!("Serve a built site, honouring the manifest's negotiation.", "\n\n", SERVE_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Serve {
        /// The directory a build wrote.
        #[arg(value_name = "DIR", default_value = "dist")]
        dir: Utf8PathBuf,

        /// Port to listen on. 0 asks the operating system for a free one.
        #[arg(short, long, default_value_t = 8787)]
        port: u16,
    },

    /// Compare two releases and report what changed.
    #[command(long_about = concat!("Compare two releases and report what changed.", "\n\n", DIFF_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Diff {
        /// The older release: a file, a directory or a glob.
        #[arg(value_name = "OLD")]
        old: String,

        /// The newer release. Add more with `--and` when a release is
        /// several files.
        #[arg(value_name = "NEW")]
        new: String,

        /// Further inputs for the newer release.
        #[arg(long = "and", value_name = "INPUT")]
        and: Vec<String>,

        /// Further inputs for the older release.
        #[arg(long = "and-old", value_name = "INPUT")]
        and_old: Vec<String>,

        /// Exit 1 when anything changed, for use in a pipeline.
        #[arg(long)]
        exit_code: bool,

        /// Exit 1 only when something breaking changed.
        #[arg(long)]
        fail_on_breaking: bool,
    },

    /// List the profiles this build knows about.
    #[command(long_about = concat!("List the profiles this build knows about.", "\n\n", PROFILES_EXAMPLES!()), after_long_help = DOCS_TRAILER)]
    Profiles,
}

/// Say so when a target carries its credentials in the command line.
///
/// clig.dev G16 asks that secrets not be read from flags. This one has to
/// be accepted -- a protected staging origin is a real thing to probe --
/// but by the time it reaches this process it is already in argv, in the
/// shell's history and visible to `ps`, and no amount of redacting the
/// output undoes that. `curl` is invoked with `--netrc-optional`, so the
/// same run works with the credential in `~/.netrc` and the URL bare.
fn warn_about_credentials_in_argv(origin: &str) {
    let has_userinfo = origin
        .split_once("//")
        .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
        .is_some_and(|authority| authority.contains('@'));
    if has_userinfo {
        eprintln!(
            "warning: {} carries a credential in the command line, where argv, \
             `ps` and the shell's history can all see it",
            http::redact_url(origin)
        );
        eprintln!("hint: put the credential in ~/.netrc and pass the URL without it");
    }
}

/// A site root has to be an absolute `http`/`https` URL.
///
/// `--base-url "not a url"` used to be accepted: the build exited 0 and
/// wrote `href="not a url&#x2f;"` into every page, so every link on the site
/// was broken and nothing said so. A base URL is interpolated into links,
/// the manifest, `llms.txt` and every host adapter, and there is no reading
/// of it that recovers from a bad one.
fn validate_base_url(url: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(crate::Failure::err(
            exit::USAGE,
            format!("--base-url {url:?} {why}"),
            "pass an absolute URL of the site root, as in https://example.org/",
        ))
    };
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return bad("is not an absolute http or https URL");
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return bad("has no host");
    }
    if authority.contains(char::is_whitespace) || rest.contains(char::is_whitespace) {
        return bad("contains a space");
    }
    if authority.contains('@') {
        return bad("carries credentials, which would be published in every link");
    }
    Ok(())
}

/// The origin of the release's root document, used when no base URL is given.
fn default_base_url(release: &model::Release) -> Option<String> {
    let iri = release.root_document().map(|d| d.iri.clone())?;
    let rest = iri.split_once("//")?.1;
    let authority = rest.split('/').next()?;
    let scheme = iri.split_once("//")?.0;
    Some(format!("{scheme}//{authority}/"))
}

/// Whether messages may carry ANSI colour (clig.dev G8).
///
/// Decided about **stderr**, which is where every coloured byte this tool
/// can emit goes: its own messages, and clap's usage errors. Deciding it
/// about stdout, as an earlier version did, dropped colour from the error
/// message whenever a report was piped into a file.
///
/// An explicit choice wins over everything, which is the part that was
/// broken: `should_color` was never called at all, so `--no-color` was a
/// no-op on the only surface in the tool that is ever coloured, and
/// `CLICOLOR_FORCE=1 iyo --no-color buld 2>log` wrote ANSI into `log`.
/// After an explicit choice come the conventional variables, in the order
/// the conventions themselves give: `NO_COLOR` and `TERM=dumb` turn colour
/// off, `CLICOLOR_FORCE` and `FORCE_COLOR` turn it on, and otherwise it
/// follows the terminal.
fn should_color(when: ColorWhen, no_color_flag: bool) -> bool {
    if no_color_flag || when == ColorWhen::Never {
        return false;
    }
    if when == ColorWhen::Always {
        return true;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return false;
    }
    if forced("CLICOLOR_FORCE") || forced("FORCE_COLOR") {
        return true;
    }
    std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// A "turn it on" variable is set and is not switched off by being `0`,
/// which is how both of these are conventionally disabled.
fn forced(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The colour decision, taken from the raw arguments before clap parses
/// them: clap colours its own usage errors, so by the time a parsed `Cli`
/// exists the decision has already been made once without it.
fn color_choice() -> clap::ColorChoice {
    let mut no_color = false;
    let mut when = ColorWhen::Auto;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-color" => no_color = true,
            "--color" => {
                if let Some(value) = args.next()
                    && let Ok(parsed) = ColorWhen::from_str(&value, true)
                {
                    when = parsed;
                }
            }
            other => {
                if let Some(value) = other.strip_prefix("--color=")
                    && let Ok(parsed) = ColorWhen::from_str(value, true)
                {
                    when = parsed;
                }
            }
        }
    }
    if should_color(when, no_color) {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Never
    }
}

pub fn main() -> ExitCode {
    // Before anything that writes or opens a socket: from here a Ctrl-C
    // sets a flag the long loops check, rather than ending the process
    // wherever it happened to be (clig.dev G23, `src/interrupt.rs`).
    interrupt::listen();
    // And die conventionally when the reader of our output goes away, rather
    // than panicking with 101 and no message (`interrupt::die_on_closed_pipe`).
    interrupt::die_on_closed_pipe();
    // `Cli::parse()` would use clap's own colour default, which ignores
    // both `--no-color` and `NO_COLOR` on the error path. Build the command
    // with the decision already made.
    let matches = Cli::command().color(color_choice()).get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => ExitCode::from(report_failure(&cli, &err)),
    }
}

/// How a failure reaches the reader, and the script behind them.
///
/// clig.dev G21 asks for a suggested fix and the most important line last,
/// so the advice goes below the diagnosis. G7 asks `--json` to stay
/// machine-readable on exactly the runs a consumer most needs to inspect:
/// before this, every command wrote nothing at all to stdout on a failure,
/// so a pipeline into a parser got an empty document and no reason.
fn report_failure(cli: &Cli, err: &anyhow::Error) -> u8 {
    let (code, hint) = crate::Failure::of(err);
    if cli.json {
        let document = serde_json::json!({
            "schema_version": model::SCHEMA_VERSION,
            "error": {
                "message": format!("{err:#}"),
                "hint": hint,
                "exit_code": code,
            }
        });
        let mut stdout = std::io::stdout();
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
        let _ = stdout.flush();
    }
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "error: {err:#}");
    let _ = writeln!(stderr, "see: {}", docs_url!());
    if let Some(hint) = hint {
        let _ = writeln!(stderr, "hint: {hint}");
    }
    code
}

fn cwd() -> Utf8PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
        .unwrap_or_else(|| Utf8PathBuf::from("."))
}

/// Compile every namespace's prepared PDF inputs in a built directory.
///
/// The inputs are part of the build and are hashed with everything else; the
/// PDF is not, because it is produced by a program the build cannot assume is
/// installed. Rebuilding gives the same inputs, and the same Typst gives the
/// same PDF from them.
fn compile_pdfs(dir: &Utf8Path, options: &pdf::Options, quiet: bool) -> Result<Vec<pdf::Built>> {
    if !pdf::available(&options.binary) {
        return Err(crate::Failure::err(
            exit::ENVIRONMENT,
            format!("{} is not on PATH", options.binary),
            "install Typst (https://typst.app/open-source/) or pass --typst-bin; \
             the prepared inputs are in <namespace>/pdf/",
        ));
    }
    // Every prepared directory, wherever it is: a versioned snapshot has one
    // of its own, and it is not listed in the manifest's namespaces.
    let mut prepared = Vec::new();
    find_pdf_inputs(dir, &mut prepared)?;
    prepared.sort();

    let mut built = Vec::new();
    for input in prepared {
        let mount = input.parent().unwrap_or(dir).to_owned();
        let model: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(input.join("model.json"))?)
                .with_context(|| format!("parsing {}", input.join("model.json")))?;
        // The build knows what the file should be called and says so, because
        // guessing from the directory picks a term's Turtle over the
        // namespace's and does not even do that the same way twice.
        let stem = model["pdf"]["stem"].as_str().unwrap_or("vocabulary");
        let output = mount.join(format!("{stem}.pdf"));

        // The vocabulary's own date, never the clock, so that rebuilding a
        // release reproduces its PDF byte for byte.
        let created = pdf::timestamp(
            model["document"]["modified"]
                .as_str()
                .or_else(|| model["document"]["created"].as_str()),
        );

        let result = pdf::compile(&input, &output, created, options)?;
        if !quiet {
            eprintln!(
                "{} ({} KiB, {}, typst {})",
                result.path,
                result.bytes / 1024,
                result.standards,
                result.typst_version
            );
        }
        built.push(result);
    }
    Ok(built)
}

/// Every directory holding a `spec.typ` the build prepared.
fn find_pdf_inputs(dir: &Utf8Path, out: &mut Vec<Utf8PathBuf>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
            continue;
        };
        if !path.is_dir() {
            continue;
        }
        if path.file_name() == Some("pdf") && path.join("spec.typ").is_file() {
            out.push(path);
            continue;
        }
        find_pdf_inputs(&path, out)?;
    }
    Ok(())
}

fn load_release(inputs: &[String], debug: bool) -> Result<(model::Release, load::Store)> {
    let base = cwd();
    let paths = load::expand_inputs(inputs, &base).context("resolving inputs")?;
    if debug {
        for path in &paths {
            eprintln!("debug: reading {path}");
        }
    }
    let store = load::load(&paths)?;
    if debug {
        for file in &store.files {
            eprintln!(
                "debug: {} triples from {} as {}",
                file.triple_count, file.path, file.format
            );
        }
    }
    let registry = profile::Registry::built_in()?;
    let release = build::build(&store, &registry)?;
    if debug {
        eprintln!(
            "debug: {} namespaces, {} documents, {} terms",
            release.namespaces.len(),
            release.documents.len(),
            release.terms.len()
        );
    }
    Ok((release, store))
}

/// The target resolution `probe` and `conform` share: a built directory, a
/// manifest file, or the URL of a deployed site, in which case
/// `manifest.json` is fetched from it and the site it came from is what gets
/// tested. Returns the manifest and the origin a caller should send requests
/// to when `--origin` was not given.
fn manifest_of_target(
    label: &str,
    target: &str,
    timeout: u32,
) -> Result<(render::manifest::Manifest, String)> {
    if target.starts_with("http") {
        // `fetch_manifest` blocks for up to `timeout` seconds before
        // `probe::run` gets a chance to print its own header, so against an
        // unresponsive host this would otherwise be exactly the silence
        // G22 asks us not to leave a person in. `target`, not just
        // `--origin`, can carry userinfo, so it is redacted too.
        eprintln!(
            "{}: fetching manifest from {}",
            label,
            http::redact_url(target)
        );
        let m = probe::fetch_manifest(target, timeout)?;
        Ok((m, target.trim_end_matches('/').to_owned()))
    } else {
        let path = Utf8PathBuf::from(target);
        let file = if path.is_dir() {
            path.join("manifest.json")
        } else {
            path
        };
        // The same answer `serve` and `pdf` give in the identical
        // situation. These two used to hand over the raw OS error, which
        // names the file that is missing and nothing about how to get one.
        let text = std::fs::read_to_string(&file).map_err(|e| {
            crate::Failure::err(
                exit::INPUT,
                format!("reading {file}: {e}"),
                format!(
                    "run `iyo build --out {}` first, or pass the directory a build wrote",
                    file.parent().unwrap_or(&file)
                ),
            )
        })?;
        let m: render::manifest::Manifest = serde_json::from_str(&text).map_err(|e| {
            crate::Failure::err(
                exit::INPUT,
                format!("parsing {file}: {e}"),
                "this file is not a manifest this version of iyo understands;                  rebuild it with `iyo build`",
            )
        })?;
        let root = m.site_root.trim_end_matches('/').to_owned();
        Ok((m, root))
    }
}

fn run(cli: &Cli) -> Result<u8> {
    match &cli.command {
        Command::Pdf {
            dir,
            pdf_standard,
            typst_bin,
            font_path,
            dry_run,
        } => {
            let options = pdf::Options {
                standards: pdf_standard.clone(),
                binary: typst_bin.clone(),
                font_paths: font_path.clone(),
            };
            if *dry_run {
                let mut prepared = Vec::new();
                find_pdf_inputs(dir, &mut prepared)?;
                prepared.sort();
                if prepared.is_empty() {
                    return Err(crate::Failure::err(
                        exit::INPUT,
                        format!("{dir} has no pdf/spec.typ"),
                        format!("run `iyo build --pdf --out {dir}` first"),
                    ));
                }
                let mut out = std::io::stdout().lock();
                if cli.json {
                    let document = serde_json::json!({
                        "schema_version": model::SCHEMA_VERSION,
                        "dry_run": true,
                        "inputs": prepared.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                    });
                    writeln!(out, "{}", serde_json::to_string_pretty(&document)?)?;
                } else if !cli.quiet {
                    for path in &prepared {
                        writeln!(out, "{path}")?;
                    }
                }
                out.flush()?;
                return Ok(exit::OK);
            }
            // The directory is checked before the environment. Pointing this
            // command at the wrong directory is the more common mistake, and
            // the answer to it does not depend on whether Typst is installed:
            // being told to install Typst, and only then that there was
            // nothing to compile, is two round trips for one error.
            let mut inputs = Vec::new();
            find_pdf_inputs(dir, &mut inputs)?;
            if inputs.is_empty() {
                return Err(crate::Failure::err(
                    exit::INPUT,
                    format!("{dir} has no pdf/spec.typ"),
                    format!("run `iyo build --pdf --out {dir}` first"),
                ));
            }
            if !pdf::available(&options.binary) {
                let mut err = std::io::stderr();
                writeln!(
                    err,
                    "error: {} is not on PATH. Install Typst \
                     (https://typst.app/open-source/) or pass --typst-bin",
                    options.binary
                )?;
                return Ok(exit::ENVIRONMENT);
            }
            let built = compile_pdfs(dir, &options, cli.quiet)?;
            if cli.json {
                let mut out = std::io::stdout().lock();
                writeln!(out, "{}", serde_json::to_string_pretty(&built)?)?;
                out.flush()?;
            }
            Ok(exit::OK)
        }

        Command::Probe {
            target,
            origin,
            sample,
            timeout,
            deadline,
            full,
            fail_on,
        } => {
            let (manifest, default_origin) = manifest_of_target("probe", target, *timeout)?;
            let origin = origin.clone().unwrap_or(default_origin);
            warn_about_credentials_in_argv(&origin);
            let report = probe::run(
                &manifest,
                &origin,
                *sample,
                *timeout,
                *full,
                Duration::from_secs(*deadline),
            )?;

            // `--json` stays on stdout, alone and parseable, whether or not
            // `--quiet` was given; the human report moves to stderr like the
            // other six subcommands that print one, and `--quiet` is what
            // suppresses it (G2).
            if cli.json {
                let mut out = std::io::stdout().lock();
                writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
                out.flush()?;
            } else if !cli.quiet {
                let mut err = std::io::stderr();
                write!(err, "{}", probe::summary(&report))?;
                err.flush()?;
            }

            // A truncated run is a case that silently did not finish, which
            // is worse than one that never ran at all: left out of `failed`
            // it reports success (`iyo probe dist --deadline 0` makes zero
            // requests and still prints "Every IRI resolved"). `conform`
            // already draws this line for `conformant()`; `probe` must hold
            // it too, under every `--fail-on` except `never`, which means
            // never regardless of cause.
            let failed = match fail_on.as_str() {
                "warning" => report.errors > 0 || report.warnings > 0 || report.truncated,
                "never" => false,
                _ => report.errors > 0 || report.truncated,
            };
            // An interrupted run reports what it gathered, and says so
            // through `truncated`, but the exit code is about why it
            // stopped: 130, not "findings". A wrapper distinguishing a
            // failing gate from an operator's Ctrl-C needs that.
            if interrupt::pending() {
                return Ok(exit::INTERRUPTED);
            }
            if failed {
                return Ok(exit::FINDINGS);
            }
            Ok(exit::OK)
        }

        Command::Conform {
            target,
            origin,
            cases,
            timeout,
            deadline,
            fail_on,
        } => {
            let (manifest, default_origin) = manifest_of_target("conform", target, *timeout)?;

            // `--cases` against a local target, with no `--origin` given, is
            // a pure compile step: write the resolved plan and stop before
            // any request is sent, so a harness can call
            // `iyo conform dist --cases plan.json` with no server running.
            // Once `--origin` is given the gate still runs, and the file is
            // written alongside it, as before.
            if let Some(path) = cases
                && origin.is_none()
                && !target.starts_with("http")
            {
                let contract = conform::cases::bundled()?;
                let plan = conform::resolve::resolve(&contract, &manifest);
                std::fs::write(path, serde_json::to_string_pretty(&plan)?)
                    .with_context(|| format!("writing {path}"))?;
                return Ok(exit::OK);
            }

            let origin = origin.clone().unwrap_or(default_origin);
            warn_about_credentials_in_argv(&origin);
            let report =
                conform::run(&manifest, &origin, *timeout, Duration::from_secs(*deadline))?;

            // Written wherever asked, never into the site output: the
            // resolved cases are a test artefact for the local harnesses,
            // not part of what gets published (`docs/cli.md`,
            // "`iyo conform`: flags").
            if let Some(path) = cases {
                let contract = conform::cases::bundled()?;
                let plan = conform::resolve::resolve(&contract, &manifest);
                std::fs::write(path, serde_json::to_string_pretty(&plan)?)
                    .with_context(|| format!("writing {path}"))?;
            }

            // `--json` stays on stdout, alone and parseable, whether or not
            // `--quiet` was given; the human report moves to stderr like the
            // other six subcommands that print one, and `--quiet` is what
            // suppresses it (G2).
            if cli.json {
                let mut out = std::io::stdout().lock();
                writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
                out.flush()?;
            } else if !cli.quiet {
                let mut err = std::io::stderr();
                write!(err, "{}", conform::summary(&report))?;
                err.flush()?;
            }

            // Unlike `probe`, `conform` has no separate warning tier: a
            // case either satisfies the convention or it does not. `error` and
            // `warning` therefore both gate on `conformant()`; only
            // `never` turns the gate off.
            let failed = match fail_on.as_str() {
                "never" => false,
                _ => !report.conformant(),
            };
            if interrupt::pending() {
                return Ok(exit::INTERRUPTED);
            }
            if failed {
                return Ok(exit::FINDINGS);
            }
            Ok(exit::OK)
        }

        Command::Serve { dir, port } => {
            if !dir.join("manifest.json").is_file() {
                return Err(crate::Failure::err(
                    exit::INPUT,
                    format!("{dir} has no manifest.json"),
                    format!("run `iyo build --out {dir}` first"),
                ));
            }
            serve::run(dir, *port, cli.quiet, cli.json)?;
            Ok(exit::OK)
        }

        Command::Diff {
            old,
            new,
            and,
            and_old,
            exit_code,
            fail_on_breaking,
        } => {
            let mut old_inputs = vec![old.clone()];
            old_inputs.extend(and_old.iter().cloned());
            let mut new_inputs = vec![new.clone()];
            new_inputs.extend(and.iter().cloned());
            let (before, _) = load_release(&old_inputs, cli.debug)?;
            let (after, _) = load_release(&new_inputs, cli.debug)?;
            let report = diff::run(&before, &after);

            let mut out = std::io::stdout().lock();
            if cli.json {
                writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
            } else {
                let title = after
                    .root_document()
                    .map(|d| d.display("en").to_owned())
                    .unwrap_or_else(|| "the vocabulary".to_owned());
                write!(out, "{}", diff::markdown(&report, &title))?;
            }
            out.flush()?;

            // A pipeline asks one of two questions: did anything change, or
            // did anything break. They deserve different answers.
            if (*fail_on_breaking && report.breaking > 0) || (*exit_code && !report.is_empty()) {
                return Ok(exit::FINDINGS);
            }
            Ok(exit::OK)
        }

        Command::Profiles => {
            let registry = profile::Registry::built_in()?;
            let mut out = std::io::stdout().lock();
            if cli.json {
                // An object, not the bare array this used to print: a
                // consumer has to be able to read `schema_version` before
                // anything else, and an array has nowhere to put it.
                let document = serde_json::json!({
                    "schema_version": model::SCHEMA_VERSION,
                    "profiles": registry
                        .ids()
                        .into_iter()
                        .map(|id| {
                            let p = registry.get(id).expect("listed profile exists");
                            serde_json::json!({ "id": p.id, "title": p.title })
                        })
                        .collect::<Vec<_>>(),
                });
                writeln!(out, "{}", serde_json::to_string_pretty(&document)?)?;
            } else {
                for id in registry.ids() {
                    let p = registry.get(id).expect("listed profile exists");
                    writeln!(out, "{:<10} {}", p.id, p.title)?;
                }
            }
            Ok(exit::OK)
        }

        Command::Model { inputs } => {
            let (release, _store) = load_release(inputs, cli.debug)?;
            let mut out = std::io::stdout().lock();
            writeln!(out, "{}", serde_json::to_string_pretty(&release)?)?;
            Ok(exit::OK)
        }

        Command::Build {
            inputs,
            out,
            base_url,
            config: config_path,
            strict,
            theme,
            snapshots,
            release: release_option,
            hosts,
            previous,
            pdf: want_pdf,
            typst_bin,
            md_frontmatter,
            link_style,
            base_path,
            theme_switch,
            color_scheme,
            dry_run,
        } => {
            let explicit = config_path.clone();
            let default_path = Utf8PathBuf::from("iyo.toml");
            let mut settings = match explicit.as_ref().or_else(|| {
                if default_path.exists() {
                    Some(&default_path)
                } else {
                    None
                }
            }) {
                Some(path) => config::Config::load(path)?,
                None => config::Config::implicit(),
            };

            let sources = if inputs.is_empty() {
                settings.inputs.clone()
            } else {
                inputs.clone()
            };
            if sources.is_empty() {
                anyhow::bail!("no inputs given and none configured");
            }
            if let Some(dir) = theme {
                // An unreadable theme used to be ignored: the build wrote a
                // default-theme site, at the default theme's digest, and
                // exited 0, so a typo in the path looked like a theme that
                // simply had no effect.
                if !dir.is_dir() {
                    return Err(crate::Failure::err(
                        exit::INPUT,
                        format!("{dir} is not a directory, so there is no theme to read"),
                        "pass --theme a directory holding templates/ and assets/, \
                         or leave it off to use the built-in theme",
                    ));
                }
                // A theme that only changes colours is a whole theme. It
                // used to be rejected until an empty `assets/` was placed
                // beside its `tokens.toml`, which is a rule about directory
                // layout masquerading as a rule about themes.
                if !dir.join("templates").is_dir()
                    && !dir.join("assets").is_dir()
                    && !dir.join("tokens.toml").is_file()
                    && !dir.join("pdf").join("spec.typ").is_file()
                {
                    return Err(crate::Failure::err(
                        exit::INPUT,
                        format!("{dir} holds nothing a theme can override"),
                        "a theme overrides the built-in one file by file: \
                         templates/ for pages, assets/ for stylesheets, \
                         tokens.toml for colour and spacing, pdf/spec.typ for \
                         the PDF",
                    ));
                }
            }
            let (release, store) = load_release(&sources, cli.debug)?;

            if let Some(url) = base_url {
                validate_base_url(url)?;
                settings.site.base_url = url.clone();
            } else if settings.site.base_url == "/"
                && let Some(derived) = default_base_url(&release)
            {
                settings.site.base_url = derived;
            }
            if let Some(when) = snapshots {
                settings.site.snapshots = when.clone();
            }
            if let Some(v) = release_option {
                settings.site.release = Some(v.clone());
            }
            if !hosts.is_empty() {
                settings.site.hosts = hosts.clone();
            }
            if !previous.is_empty() {
                settings.site.previous = previous.clone();
            }
            if *want_pdf {
                settings.site.pdf = true;
            }
            if let Some(style) = md_frontmatter {
                settings.site.md_frontmatter = style.clone();
            }
            if let Some(style) = link_style {
                settings.site.link_style = style.clone();
            }
            if let Some(path) = base_path {
                settings.site.base_path = Some(path.clone());
            }
            if *theme_switch {
                settings.site.theme_switch = true;
            }
            if let Some(scheme) = color_scheme {
                settings.site.color_scheme = scheme.clone();
            }
            // A control with nothing to control. Rejected rather than
            // ignored: a flag that silently does nothing is the defect this
            // project has fixed four times.
            if settings.site.theme_switch && settings.site.color_scheme != "auto" {
                return Err(crate::Failure::err(
                    exit::USAGE,
                    format!(
                        "--theme-switch offers a choice between schemes, and \
                         --color-scheme {} publishes only one",
                        settings.site.color_scheme
                    ),
                    "drop one of the two: --color-scheme auto to offer both, \
                     or no --theme-switch to publish the one",
                ));
            }
            settings.normalise();

            let report = check::run(&release, &store, &check::Options::default());
            if report.summary.errors > 0 || (*strict && report.summary.warnings > 0) {
                let mut err = std::io::stderr();
                for f in &report.findings {
                    if f.severity == check::Severity::Error || *strict {
                        writeln!(
                            err,
                            "{}  {:<7}  {}  [{}]",
                            f.file.as_deref().or(f.subject.as_deref()).unwrap_or("-"),
                            f.severity.as_str(),
                            f.message,
                            f.rule
                        )?;
                    }
                }
                writeln!(
                    err,
                    "refusing to build: run `iyo check` for the full report"
                )?;
                return Ok(exit::FINDINGS);
            }

            // A changelog needs the release before this one. It is loaded
            // here rather than in the renderer, which reads no files.
            let previous_release = if settings.site.previous.is_empty() {
                None
            } else {
                Some(load_release(&settings.site.previous, cli.debug)?)
            };
            let changes = previous_release
                .as_ref()
                .map(|(before, _)| diff::run(before, &release));

            let plan = site::Plan::new(&release, &settings);
            let ctx = render::Ctx {
                release: &release,
                store: &store,
                plan: &plan,
                config: &settings,
                changes: changes.as_ref(),
            };
            let output = render::render_with(&ctx, theme.clone())?;
            let digest = output.digest();

            // The tool audits its own HTML. These findings are defects in the
            // theme or in the vocabulary's own names, not opinions, so they
            // stop the build rather than land in a report nobody reads.
            if output.errors() > 0 || output.contrast_failures > 0 || output.contrast_errors > 0 {
                let mut err = std::io::stderr();
                for issue in output
                    .issues
                    .iter()
                    .filter(|i| i.level == render::audit::Level::Error)
                    .take(20)
                {
                    writeln!(
                        err,
                        "{}  error    {}  [{}, {}]",
                        issue.page, issue.message, issue.rule, issue.criterion
                    )?;
                }
                for finding in output
                    .contrast_findings
                    .iter()
                    .filter(|f| f.severity == crate::theme::Severity::Error)
                    .filter(|f| !f.rule.starts_with("theme.contrast-"))
                {
                    writeln!(err, "{}  [{}]", finding.message, finding.rule)?;
                }
                if output.contrast_failures > 0 {
                    writeln!(
                        err,
                        "{} colour pairs in the theme fail their contrast requirement",
                        output.contrast_failures
                    )?;
                    // Which ones, and by how much. The gate computes all of
                    // this; printing only the count left a theme author
                    // guessing which of thirteen roles in two schemes to
                    // move, which is most of the work of writing a theme.
                    for pair in output.contrast_failed.iter().take(20) {
                        writeln!(
                            err,
                            "  {} {} on {} is {}:1, needs {:.1}:1  ({})",
                            pair.scheme,
                            pair.foreground,
                            pair.background,
                            pair.ratio_2dp,
                            pair.required,
                            pair.site
                        )?;
                        writeln!(err, "      {} on {}", pair.fg_value, pair.bg_value)?;
                    }
                    if output.contrast_failed.len() > 20 {
                        writeln!(err, "  ... and {} more", output.contrast_failed.len() - 20)?;
                    }
                }
                // Name what actually stopped the build. This line used to
                // read "refusing to write: 0 accessibility errors in the
                // rendered pages" whenever the contrast gate alone fired,
                // which is a sentence that says the reason is nothing.
                let mut causes = Vec::new();
                if output.errors() > 0 {
                    causes.push(format!(
                        "{} accessibility {} in the rendered pages",
                        output.errors(),
                        if output.errors() == 1 {
                            "error"
                        } else {
                            "errors"
                        }
                    ));
                }
                if output.contrast_failures > 0 {
                    causes.push(format!(
                        "{} failing colour {}",
                        output.contrast_failures,
                        if output.contrast_failures == 1 {
                            "pair"
                        } else {
                            "pairs"
                        }
                    ));
                }
                if output.contrast_errors > 0 {
                    causes.push(format!(
                        "{} unreadable theme {}",
                        output.contrast_errors,
                        if output.contrast_errors == 1 {
                            "token"
                        } else {
                            "tokens"
                        }
                    ));
                }
                writeln!(err, "refusing to write: {}", causes.join(" and "))?;
                return Ok(exit::FINDINGS);
            }

            let output_warnings = output
                .issues
                .iter()
                .filter(|i| i.level == render::audit::Level::Warning)
                .count();
            // Warnings used to be a number and nothing else, which is the
            // same defect the contrast gate had: a count tells a publisher
            // that something is wrong and not what. Grouped by rule, because
            // 2,000 warnings are rarely 2,000 problems -- on a real run they
            // were six links repeated across 233 pages.
            if output_warnings > 0 && !cli.quiet && !cli.json {
                let mut by_rule: std::collections::BTreeMap<&str, (usize, &str, &str)> =
                    std::collections::BTreeMap::new();
                for issue in output
                    .issues
                    .iter()
                    .filter(|i| i.level == render::audit::Level::Warning)
                {
                    let entry = by_rule.entry(issue.rule.as_str()).or_insert((
                        0,
                        issue.message.as_str(),
                        issue.page.as_str(),
                    ));
                    entry.0 += 1;
                }
                let mut err = std::io::stderr();
                for (rule, (count, example, page)) in &by_rule {
                    writeln!(
                        err,
                        "warning  {rule}: {count} across the site, e.g. {page}: {example}"
                    )?;
                }
            }
            let notes = output.notes.clone();
            let files = if *dry_run {
                output.planned()
            } else {
                output.write(out)?
            };

            let mut counts = std::collections::BTreeMap::new();
            counts.insert("documents".to_owned(), release.stats.documents);
            counts.insert("terms".to_owned(), release.stats.terms_local);
            counts.insert("namespaces".to_owned(), plan.namespaces.len());
            counts.insert("files".to_owned(), files.len());
            let build_report = render::BuildReport {
                schema_version: model::SCHEMA_VERSION,
                out_dir: out.to_string(),
                notes,
                files,
                digest,
                counts,
            };

            let mut stdout = std::io::stdout().lock();
            if cli.json {
                writeln!(stdout, "{}", serde_json::to_string_pretty(&build_report)?)?;
            } else if !cli.quiet {
                let mut err = std::io::stderr();
                writeln!(
                    err,
                    "{} files, {} documents, {} terms in {} namespaces -> {}",
                    build_report.files.len(),
                    release.stats.documents,
                    release.stats.terms_local,
                    plan.namespaces.len(),
                    out
                )?;
                writeln!(err, "digest {}", build_report.digest)?;
                writeln!(
                    err,
                    "{} pages audited, 0 errors, {} warnings; theme contrast checked",
                    build_report
                        .files
                        .iter()
                        .filter(|f| f.path.ends_with(".html"))
                        .count(),
                    output_warnings
                )?;
                for note in &build_report.notes {
                    writeln!(err, "note: {note}")?;
                }
                if *dry_run {
                    writeln!(err, "dry run: nothing was written")?;
                }
            }

            // Compiling the PDF comes last and separately: the site is on
            // disk and usable whatever Typst does, and a missing Typst is a
            // problem with the environment rather than with the build.
            if settings.site.pdf && !*dry_run {
                let options = pdf::Options {
                    binary: typst_bin.clone(),
                    ..pdf::Options::default()
                };
                match compile_pdfs(out, &options, cli.quiet) {
                    Ok(_) => {}
                    Err(e) => {
                        // Exit 4 is right: --pdf asked for a PDF and there
                        // is none. What was wrong was saying the same thing
                        // twice, once here and once in the error's own
                        // hint, in two different wordings.
                        let mut err = std::io::stderr();
                        writeln!(err, "the site was written; the PDF was not: {e}")?;
                        writeln!(
                            err,
                            "hint: compile the prepared inputs later with `iyo pdf {out}`, \
                             or install Typst (https://typst.app/open-source/)"
                        )?;
                        return Ok(exit::ENVIRONMENT);
                    }
                }
            }
            Ok(exit::OK)
        }

        Command::Check {
            inputs,
            strict,
            strict_lang,
            select,
            ignore,
        } => {
            // A filter that matches no rule used to run silently and report
            // nothing, so a misspelling in CI produced a green job that had
            // checked nothing at all. It is a usage error, like any other
            // unrecognised value.
            let unknown: Vec<String> = check::unknown_filters(select)
                .into_iter()
                .chain(check::unknown_filters(ignore))
                .collect();
            if let Some(first) = unknown.first() {
                let near: Vec<&str> = check::RULES
                    .iter()
                    .copied()
                    .filter(|r| {
                        let head = first.split('.').next().unwrap_or(first);
                        r.starts_with(head)
                    })
                    .take(4)
                    .collect();
                return Err(crate::Failure::err(
                    exit::USAGE,
                    format!("no rule matches {first:?}"),
                    if near.is_empty() {
                        "a filter is a rule id or a prefix of one; the families are \
                         header. release. site. term. text."
                            .to_owned()
                    } else {
                        format!("did you mean one of: {}", near.join(", "))
                    },
                ));
            }
            let (release, store) = load_release(inputs, cli.debug)?;
            let options = check::Options {
                select: select.clone(),
                ignore: ignore.clone(),
                strict_lang: *strict_lang,
            };
            let report = check::run(&release, &store, &options);

            let mut out = std::io::stdout().lock();
            if cli.json {
                writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
            } else {
                for f in &report.findings {
                    let where_ = f.file.as_deref().or(f.subject.as_deref()).unwrap_or("-");
                    writeln!(
                        out,
                        "{where_}  {:<7}  {}  [{}]",
                        f.severity.as_str(),
                        f.message,
                        f.rule
                    )?;
                }
            }
            out.flush()?;

            if !cli.quiet {
                let s = &report.summary;
                let mut err = std::io::stderr();
                writeln!(
                    err,
                    "{} documents, {} local terms, {} reused, {} triples from {} files",
                    release.stats.documents,
                    release.stats.terms_local,
                    release.stats.terms_foreign,
                    release.stats.triples,
                    release.stats.files
                )?;
                writeln!(
                    err,
                    "{} errors, {} warnings, {} info",
                    s.errors, s.warnings, s.info
                )?;
                // --ignore can hide error-level rules, so a run that passes
                // because of it should say so rather than look clean.
                if s.suppressed > 0 {
                    writeln!(
                        err,
                        "{} finding{} suppressed by --ignore",
                        s.suppressed,
                        if s.suppressed == 1 { "" } else { "s" }
                    )?;
                }
            }

            if report.summary.errors > 0 || (*strict && report.summary.warnings > 0) {
                Ok(exit::FINDINGS)
            } else {
                Ok(exit::OK)
            }
        }
    }
}
