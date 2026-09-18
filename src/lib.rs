//! `iyo` publishes OWL, RDFS, SKOS and SHACL vocabularies as a static site.
//!
//! The crate is organised as a pipeline: load,
//! partition, model, enrich, check, render, verify. Every stage reads only the
//! stage before it, and renderers see the model, never RDF.
//!
//! The name is the historic province that is now Ehime, where the tool was
//! started at BioHackathon 2026 in Matsuyama.

pub mod adapter;
pub mod build;
pub mod check;
pub mod cli;
pub mod config;
pub mod conform;
pub mod diff;
pub mod http;
pub mod interrupt;
pub mod load;
pub mod model;
pub mod negotiate;
pub mod pdf;
pub mod probe;
pub mod profile;
pub mod render;
pub mod serve;
pub mod shape;
pub mod site;
pub mod theme;
pub mod version;
pub mod vocab;

/// Exit codes, documented in `docs/cli.md` under "Exit codes".
pub mod exit {
    /// Success; no findings at error level.
    pub const OK: u8 = 0;
    /// Findings at error level, or warnings under `--strict`.
    pub const FINDINGS: u8 = 1;
    /// Usage error. Emitted by clap.
    pub const USAGE: u8 = 2;
    /// Input could not be read or parsed.
    pub const INPUT: u8 = 3;
    /// Environment error: a required external tool is missing.
    pub const ENVIRONMENT: u8 = 4;
    /// Output directory conflict or I/O failure.
    pub const IO: u8 = 5;
    /// Interrupted. The same number a shell synthesises for a process killed
    /// by SIGINT (128 + 2), so a wrapper sees one code either way.
    pub const INTERRUPTED: u8 = crate::interrupt::EXIT_INTERRUPTED;
}

/// An error that knows which documented exit code it is, and what the reader
/// should do about it.
///
/// Without this every error path exited 3 (`INPUT`), so a usage mistake and
/// an unreadable file were indistinguishable to a script, and `IO` was a
/// code the table documented and nothing produced. clig.dev G21 also asks
/// for a suggested fix; `hint` carries it, and `cli::main` prints it last,
/// where the eye lands.
#[derive(Debug)]
pub struct Failure {
    pub code: u8,
    pub message: String,
    /// What to do about it, in the imperative. One line.
    pub hint: Option<String>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

impl Failure {
    /// An error with a documented code and a suggested fix.
    pub fn new(code: u8, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    /// The same, as an `anyhow::Error`, which is what every call site wants:
    /// these are returned from functions that already deal in `Result<_,
    /// anyhow::Error>`.
    pub fn err(code: u8, message: impl Into<String>, hint: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(Self::new(code, message, hint))
    }

    /// The code and hint carried by an error, if it carries any. An error
    /// from anywhere else is an input error with no advice, which is what
    /// every error used to be.
    pub fn of(err: &anyhow::Error) -> (u8, Option<&str>) {
        match err.downcast_ref::<Self>() {
            Some(f) => (f.code, f.hint.as_deref()),
            None => (exit::INPUT, None),
        }
    }
}
