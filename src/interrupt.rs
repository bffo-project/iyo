//! Ctrl-C, without a dependency.
//!
//! clig.dev G23 asks a program to exit promptly on an interrupt, with bounded
//! cleanup and a crash-only design. `build` had the crash-only half already:
//! it stages into a sibling and swaps with a rename, so `--out` is never
//! half written whatever happens. What it did not have was the other two.
//! An interrupt killed the process by default disposition, leaving
//! `<out>.iyo-partial-<pid>` behind for the next build to find and name, and
//! the 130 a wrapper saw was the shell synthesising 128 + SIGINT rather than
//! anything this program chose.
//!
//! No dependency was the reason. `libc` is not in `Cargo.toml` and adding one
//! for two symbols is not a trade this project makes: the dependency count
//! stays small. But `signal` and `raise` are in libc, libc is
//! linked into every Unix Rust binary already, and declaring the two
//! prototypes is a few lines. That is what this module is.
//!
//! **What a handler may do.** Only async-signal-safe things, which rules out
//! allocating, formatting, locking and touching the filesystem. This one
//! stores a flag and reinstates the default disposition, both on POSIX's
//! list. Everything else -- deciding, cleaning up, reporting -- happens on
//! the main thread, which polls [`pending`] at points where stopping is
//! safe. A second Ctrl-C therefore kills immediately, by the default
//! disposition, so a run wedged somewhere that never polls is still
//! killable.
//!
//! **Where polling happens** is the part that decides "promptly": between
//! files in `render::Output::write_into`, and between request batches in
//! `probe` and `conform`. A `build` of the reference ontology writes ~1,000
//! small files, so the gap between polls is sub-millisecond; a probe's gap is
//! one batch, and the terminal sends SIGINT to the whole foreground process
//! group, so the `curl` child dies at the same moment rather than holding the
//! run open for its timeout.
//!
//! On a platform that is not Unix, [`listen`] does nothing and [`pending`] is
//! always false: the process keeps whatever behaviour the platform gives it,
//! which is what it had before this module existed.

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The exit code for an interrupted run, and the one a shell synthesises for
/// a process killed by SIGINT (128 + 2), so a wrapper sees the same number
/// whether the handler ran or the second Ctrl-C did.
pub const EXIT_INTERRUPTED: u8 = 130;

#[cfg(unix)]
mod sys {
    // `signal(2)`, declared rather than depended on: libc is already linked
    // into this binary and the crate would be a dependency for one symbol.
    //
    // The handler argument is a `usize` rather than a function pointer so
    // that `SIG_DFL` (0) can be passed without a cast at the call site.
    unsafe extern "C" {
        pub fn signal(signum: i32, handler: usize) -> usize;
    }

    /// Interrupt from the keyboard. 2 on every Unix this builds for.
    pub const SIGINT: i32 = 2;
    /// Write to a pipe with no reader. 13 on every Unix this builds for.
    pub const SIGPIPE: i32 = 13;
    /// The default disposition: terminate.
    pub const SIG_DFL: usize = 0;
    /// What `signal` returns when it could not install the handler.
    pub const SIG_ERR: usize = usize::MAX;
}

#[cfg(unix)]
extern "C" fn on_interrupt(signum: i32) {
    INTERRUPTED.store(true, Ordering::SeqCst);
    // Put the default disposition back, so a second Ctrl-C kills the process
    // outright. A run stuck somewhere that never polls stays killable, which
    // is the property a handler most easily takes away.
    unsafe { sys::signal(signum, sys::SIG_DFL) };
}

/// Ask for SIGINT to set a flag rather than end the process.
///
/// Call once, early. Returns whether the handler was installed: `false` on a
/// platform without signals, or if the C call refused, in which case the
/// process keeps the default disposition and every `pending` check is simply
/// always false. Nothing downstream needs to branch on it.
pub fn listen() -> bool {
    #[cfg(unix)]
    {
        let handler = on_interrupt as extern "C" fn(i32) as usize;
        let previous = unsafe { sys::signal(sys::SIGINT, handler) };
        previous != sys::SIG_ERR
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Die the way every other Unix tool dies when the thing reading us stops.
///
/// The Rust runtime sets `SIGPIPE` to `SIG_IGN` before `main`, so a write to
/// a closed pipe returns `EPIPE` instead of ending the process. `println!`
/// and `eprintln!` then **panic**, because their expansion ends in
/// `.expect("failed printing to stdout")`. The result was the worst of both:
///
/// ```text
/// $ iyo build … --pdf 2>&1 | head -4
/// iyo exit: 101      # a panic
/// pdfs: 2 of 11      # and no message saying so, because the panic
///                    # message goes to the pipe that just closed
/// ```
///
/// Exit 101 is undocumented, unactionable, and indistinguishable from a real
/// crash. Restoring the default disposition makes the same command exit 141
/// (128 + SIGPIPE) quietly, which is what `find / | head` and `yes | head`
/// do and what a shell and a reader both already understand.
///
/// It does not make the truncation harmless, and nothing could: `| head` asks
/// for the producer to stop, so the PDF stage stops with fewer PDFs than
/// asked for. What changes is that stopping stops looking like a crash. The
/// site itself is never affected, because it is written and swapped before
/// the PDF stage runs at all.
pub fn die_on_closed_pipe() {
    #[cfg(unix)]
    unsafe {
        sys::signal(sys::SIGPIPE, sys::SIG_DFL);
    }
}

/// Whether an interrupt has been seen. Cheap enough to call in a tight loop.
pub fn pending() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// `Err` with a uniform message once an interrupt has been seen, so a caller
/// can `?` out of a loop at a point where stopping is safe.
pub fn check() -> anyhow::Result<()> {
    if pending() {
        return Err(crate::Failure::err(
            EXIT_INTERRUPTED,
            "interrupted",
            "nothing was left half written; run the same command again when ready",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag starts clear, and `check` is `Ok` until something sets it.
    /// Deliberately does not raise a signal: these tests run in-process
    /// beside every other test, and a real SIGINT would take the whole test
    /// binary down. The end-to-end behaviour is exercised by
    /// `tests/clig.rs::g23_*`, which sends a real signal to a real child.
    #[test]
    fn nothing_is_pending_until_a_signal_arrives() {
        assert!(!pending());
        assert!(check().is_ok());
    }

    /// Installing the handler twice is what a second `listen` would do, and
    /// it must not report failure: `main` calls it once, but a library user
    /// embedding this crate might not.
    #[test]
    #[cfg(unix)]
    fn listening_is_idempotent() {
        assert!(listen());
        assert!(listen());
        // And it did not set the flag as a side effect.
        assert!(!pending());
        // Put the default back so the rest of the suite is unaffected.
        unsafe { sys::signal(sys::SIGINT, sys::SIG_DFL) };
    }
}
