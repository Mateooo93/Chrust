// rustc-wrap: cargo rustc-wrapper that hands most compilations to sccache.
//
// Windows caps a process command line at 32767 characters, and some crates here
// (gpui-component, the browser itself) go well past that. Cargo only uses its
// own response-file fallback when no wrapper is configured, and sccache
// re-serialises the argument list, so both fail with
// `failed to spawn ... (exit code: 0xfffffffe)`.
//
// So: short command lines go through sccache (cacheable), long ones are written
// to a temporary rustc response file and compiled straight by rustc.
//
// Usage (cargo does this automatically): rustc-wrap <rustc> <args...>

use std::env;
use std::fs;
use std::process::{exit, Command};

/// Stay safely under the 32767-character command-line limit. Spawners escape
/// backslashes (doubling them) and quote every argument, so a raw argument list
/// well under the cap can still overflow once handed to sccache or CreateProcess.
const MAX_CACHED_LEN: usize = 24_000;

fn sccache_path() -> Option<std::path::PathBuf> {
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"))?;
    let mut p = std::path::PathBuf::from(home);
    p.push(".cargo");
    p.push("bin");
    p.push("sccache.exe");
    p.exists().then_some(p)
}

fn response_file(name_hint: &str, args: &[String]) -> Option<std::path::PathBuf> {
    let dir = env::var_os("TEMP")
        .or_else(|| env::var_os("TMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    // One argument per line: rustc splits response files on newlines and does
    // no quoting, which is exactly what we want for paths with spaces.
    let mut body = String::with_capacity(args.iter().map(|a| a.len() + 1).sum());
    for a in args {
        body.push_str(a);
        body.push('\n');
    }
    let file = dir.join(format!(
        "rustc-wrap-{}-{}.rsp",
        std::process::id(),
        name_hint
    ));
    fs::write(&file, body).ok()?;
    Some(file)
}

fn main() {
    let mut args: Vec<String> = env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.is_empty() {
        eprintln!("rustc-wrap: expected the compiler path as the first argument");
        exit(2);
    }
    let rustc = args.remove(0);

    // Command line length as a spawner sees it: every argument quoted, every
    // backslash doubled, plus the executable path and the launcher in front.
    let len: usize = args
        .iter()
        .map(|a| a.len() + a.matches('\\').count() + 3)
        .sum::<usize>()
        + 512;

    // Name the response file after the crate, to make leftovers diagnosable.
    let hint = args
        .windows(2)
        .find(|w| w[0] == "--crate-name")
        .map(|w| {
            w[1].chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        })
        .unwrap_or_else(|| "crate".to_string());

    let debug = env::var_os("RUSTC_WRAP_DEBUG").is_some();
    // Cargo already switched to a response file when the real command line
    // would be too long: hand that straight to rustc. sccache would expand the
    // file and then fail to spawn the compiler with the oversized line.
    let pre_expanded = args.iter().any(|a| a.starts_with('@'));
    let mut cleanup: Option<std::path::PathBuf> = None;
    let mut cmd = if !pre_expanded && len < MAX_CACHED_LEN {
        match sccache_path() {
            Some(sccache) => {
                let mut c = Command::new(sccache);
                c.arg(&rustc);
                c
            }
            None => Command::new(&rustc),
        }
    } else if pre_expanded {
        Command::new(&rustc)
    } else {
        match response_file(&hint, &args) {
            Some(rsp) => {
                let mut c = Command::new(&rustc);
                c.arg(format!("@{}", rsp.display()));
                cleanup = Some(rsp);
                c
            }
            None => Command::new(&rustc),
        }
    };
    if cleanup.is_none() {
        cmd.args(&args);
    }
    if debug {
        eprintln!(
            "rustc-wrap: {} (len {}, {})",
            hint,
            len,
            match (&cleanup, pre_expanded) {
                (Some(_), _) => "response file",
                (None, true) => "cargo response file, direct",
                (None, false) => "sccache",
            }
        );
    }

    // Children inherit our stdio, so diagnostics keep streaming to cargo.
    let status = match cmd.status() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("rustc-wrap: failed to run {rustc}: {err}");
            if let Some(rsp) = cleanup {
                let _ = fs::remove_file(rsp);
            }
            exit(1);
        }
    };
    if let Some(rsp) = cleanup {
        let _ = fs::remove_file(rsp);
    }
    exit(status.code().unwrap_or(1));
}
