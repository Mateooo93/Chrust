//! Start-up timings in the log, so "it feels faster" can be checked against
//! numbers instead of guessed at.
//!
//! Every run writes a few `…ms` lines to `chrust.log`: when the window existed,
//! when the first frame was drawn, when the first page was ready. Set
//! `CHRUST_SELFTEST=<milliseconds>` to have the browser write those timings and
//! then close itself, which is how start-up changes are measured without a
//! stopwatch.

use std::collections::HashSet;
use std::cell::RefCell;
use std::time::Instant;

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

thread_local! {
    /// Milestones already written, so a per-frame call site logs once.
    static SEEN: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
}

/// Records the process start and, when asked, schedules the self-test's exit.
pub fn start() {
    let _ = START.get_or_init(Instant::now);
    let Ok(setting) = std::env::var("CHRUST_SELFTEST") else {
        return;
    };
    let ms: u64 = setting.trim().parse().unwrap_or(4000).max(1);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        log::info!("self-test finished after {}ms", elapsed_ms());
        log::logger().flush();
        std::process::exit(0);
    });
}

/// Milliseconds since the process started.
pub fn elapsed_ms() -> u128 {
    START.get().map_or(0, |start| start.elapsed().as_millis())
}

/// Writes `label` and how long the process has been running.
pub fn mark(label: impl std::fmt::Display) {
    log::info!("{label}: {}ms", elapsed_ms());
}

/// Writes `label` the first time it is reached, and never again.
pub fn mark_once(label: &'static str) {
    let first = SEEN.with(|seen| seen.borrow_mut().insert(label));
    if first {
        mark(label);
    }
}
