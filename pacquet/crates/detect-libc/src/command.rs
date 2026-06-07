use std::process::Command;

use crate::Implementation;

/// Run `getconf GNU_LIBC_VERSION`, falling back to `ldd --version`.
///
/// `getconf` reliably identifies glibc; on musl it fails (the
/// variable doesn't exist), so `ldd --version` picks it up.
pub fn detect() -> Option<Implementation> {
    run_getconf().or_else(run_ldd)
}

fn run_getconf() -> Option<Implementation> {
    let output = Command::new("getconf").arg("GNU_LIBC_VERSION").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    parse_getconf(&stdout)
}

fn parse_getconf(stdout: &str) -> Option<Implementation> {
    if stdout.contains("glibc") {
        return Some(Implementation::Glibc);
    }
    if stdout.contains("musl") {
        return Some(Implementation::Musl);
    }
    None
}

fn run_ldd() -> Option<Implementation> {
    let output = Command::new("ldd").arg("--version").output().ok()?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    parse_ldd(&combined)
}

fn parse_ldd(combined: &str) -> Option<Implementation> {
    if combined.contains("musl") {
        return Some(Implementation::Musl);
    }
    if combined.contains("glibc")
        || combined.contains("GLIBC")
        || combined.contains("GNU C Library")
        || combined.contains("GNU libc")
    {
        return Some(Implementation::Glibc);
    }
    None
}

#[cfg(test)]
mod tests;
