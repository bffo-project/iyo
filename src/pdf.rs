//! A PDF of each namespace, through Typst.
//!
//! Two decisions shape this and both are about who owns the output.
//!
//! The host language generates no Typst markup. The build writes `model.json`
//! and a `spec.typ` that reads it, and Typst does the rest. A publisher
//! themes the PDF by editing one `.typ` file, which is the same relationship
//! the HTML has with CSS: if theming meant patching Rust, nobody would theme
//! anything.
//!
//! Typst runs as a subprocess rather than in process. Linking it would add
//! about 33 MB to the binary, raise the minimum Rust version from 1.87 to
//! 1.92, and add minutes to a cold build, all for an output most builds do
//! not ask for. The cost is that `typst` has to be installed,
//! which the command checks for and says how to fix.
//!
//! The PDF is asked to conform to a standard rather than merely to exist.
//! PDF/A-2a requires tagged structure, which is what makes a screen reader
//! able to follow a heading, and PDF/UA-1 states the accessibility claim
//! outright. Typst gained `ua-1` in 0.15, so the version is read and the
//! request is what the installed toolchain can actually honour: claiming
//! PDF/UA on a file that is not tagged for it would be worse than not
//! claiming it.

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

/// What `iyo pdf` was asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    /// PDF standards to enforce, or `None` to pick by Typst's version.
    pub standards: Option<String>,
    /// Where `typst` is, when it is not on `PATH`.
    pub binary: String,
    /// Extra font directories. Empty means Typst's embedded fonts only,
    /// which is what keeps the output the same on every machine.
    pub font_paths: Vec<Utf8PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            standards: None,
            binary: "typst".to_owned(),
            font_paths: Vec::new(),
        }
    }
}

/// One namespace's PDF, and how it was produced.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Built {
    pub namespace: String,
    pub path: String,
    pub bytes: u64,
    /// The standards actually requested, which may be fewer than asked for.
    pub standards: String,
    pub typst_version: String,
}

/// Whether the Typst binary can be run at all.
pub fn available(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The Typst version, as `major.minor.patch`.
fn version(binary: &str) -> Result<(u32, u32, String)> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run {binary:?} ({e}). Install Typst (https://typst.app/open-source/) \
             or pass --typst-bin"
            )
        })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let number = text
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or("0.0.0")
        .to_owned();
    let mut parts = number.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Ok((major, minor, number))
}

/// The standards to ask for, given what this Typst understands.
///
/// `ua-1` is the accessibility claim and arrived in Typst 0.15. Asking an
/// older binary for it fails the whole compile, so an older binary is asked
/// for PDF/A-2a alone, which still requires the tagged structure a screen
/// reader needs.
fn standards_for(major: u32, minor: u32) -> &'static str {
    if (major, minor) >= (0, 15) {
        "ua-1,a-2a"
    } else {
        "a-2a"
    }
}

/// Compile one namespace's prepared directory.
///
/// `dir` holds `spec.typ` and `model.json`, which the build wrote.
pub fn compile(
    dir: &Utf8Path,
    output: &Utf8Path,
    created: Option<i64>,
    options: &Options,
) -> Result<Built> {
    let (major, minor, number) = version(&options.binary)?;
    let standards = options
        .standards
        .clone()
        .unwrap_or_else(|| standards_for(major, minor).to_owned());

    let mut command = Command::new(&options.binary);
    command.arg("compile");
    command.args(["--root", dir.as_str()]);
    // Embedded fonts only unless the publisher supplies some, so that two
    // machines produce the same file.
    command.arg("--ignore-system-fonts");
    for path in &options.font_paths {
        command.args(["--font-path", path.as_str()]);
    }
    command.args(["--pdf-standard", &standards]);
    // Never the clock: the date comes from the vocabulary's own metadata, so
    // that rebuilding a release reproduces its PDF. Epoch 0 when the RDF
    // records no date at all, which PDF/A requires a value for.
    command.args(["--creation-timestamp", &created.unwrap_or(0).to_string()]);
    command.arg(dir.join("spec.typ").as_str());
    command.arg(output.as_str());

    let result = command
        .output()
        .with_context(|| format!("running {}", options.binary))?;
    if !result.status.success() {
        let message = String::from_utf8_lossy(&result.stderr);
        bail!(
            "typst {number} could not compile {}:\n{}",
            dir.join("spec.typ"),
            message.trim()
        );
    }

    let bytes = std::fs::metadata(output)
        .with_context(|| format!("{output} was not written"))?
        .len();
    Ok(Built {
        namespace: dir.to_string(),
        path: output.to_string(),
        bytes,
        standards,
        typst_version: number,
    })
}

/// A date in the RDF, as a UNIX timestamp, for the PDF's creation date.
///
/// Only a plain `YYYY-MM-DD` is read. Anything else would need a date library
/// for a field whose only job is to be stable, and a wrong guess is worse
/// than the epoch.
pub fn timestamp(date: Option<&str>) -> Option<i64> {
    let date = date?;
    let mut parts = date.get(..10)?.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return None;
    }
    // Days since the epoch by the civil-from-days algorithm, which needs no
    // dependency and no table.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_becomes_a_stable_timestamp() {
        // Expected values computed independently, with Python's datetime.
        assert_eq!(timestamp(Some("1970-01-01")), Some(0));
        assert_eq!(timestamp(Some("2000-03-01")), Some(951_868_800));
        assert_eq!(timestamp(Some("2026-04-28")), Some(1_777_334_400));
        // A datetime is read as its date; the time of day is not the point.
        assert_eq!(timestamp(Some("2026-04-28T13:45:00Z")), Some(1_777_334_400));
        // Anything unreadable is refused rather than guessed at.
        assert_eq!(timestamp(Some("last Tuesday")), None);
        assert_eq!(timestamp(Some("2026-13-01")), None);
        assert_eq!(timestamp(None), None);
    }

    #[test]
    fn the_accessibility_standard_is_only_claimed_when_typst_can_honour_it() {
        assert_eq!(standards_for(0, 14), "a-2a");
        assert_eq!(standards_for(0, 15), "ua-1,a-2a");
        assert_eq!(standards_for(1, 0), "ua-1,a-2a");
    }
}
