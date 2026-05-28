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
mod tests {
    use super::{parse_getconf, parse_ldd};
    use crate::Implementation;

    #[test]
    fn getconf_glibc() {
        assert_eq!(parse_getconf("glibc 2.42\n"), Some(Implementation::Glibc));
    }

    #[test]
    fn getconf_musl() {
        assert_eq!(parse_getconf("musl libc (x86_64)\n"), Some(Implementation::Musl));
    }

    #[test]
    fn getconf_unknown() {
        assert_eq!(parse_getconf("some garbage\n"), None);
    }

    #[test]
    fn getconf_empty() {
        assert_eq!(parse_getconf(""), None);
    }

    #[test]
    fn getconf_binary_noise() {
        let input = String::from_utf8_lossy(b"glibc\xFF2.42\n").into_owned();
        assert_eq!(parse_getconf(&input), Some(Implementation::Glibc));
    }

    #[test]
    fn ldd_binary_noise_musl() {
        let input = String::from_utf8_lossy(b"musl\xFFlibc\nVersion 1.2.3\n").into_owned();
        assert_eq!(parse_ldd(&input), Some(Implementation::Musl));
    }

    #[test]
    fn ldd_binary_noise_glibc() {
        let input = String::from_utf8_lossy(b"GNU C Library\xFF(glibc) 2.42\n").into_owned();
        assert_eq!(parse_ldd(&input), Some(Implementation::Glibc));
    }

    #[test]
    fn ldd_glibc_lowercase() {
        assert_eq!(parse_ldd("ldd (glibc 2.42)\n"), Some(Implementation::Glibc));
    }

    #[test]
    fn ldd_glibc_uppercase() {
        assert_eq!(
            parse_ldd("ldd (Ubuntu GLIBC 2.42-0ubuntu9) 2.42\n"),
            Some(Implementation::Glibc),
        );
    }

    #[test]
    fn ldd_glibc_gnu_c_library() {
        assert_eq!(parse_ldd("GNU C Library (glibc) 2.42\n"), Some(Implementation::Glibc));
    }

    #[test]
    fn ldd_glibc_gnu_libc() {
        assert_eq!(parse_ldd("GNU libc 2.42\n"), Some(Implementation::Glibc));
    }

    #[test]
    fn ldd_musl() {
        assert_eq!(parse_ldd("musl libc (x86_64)\nVersion 1.2.3\n"), Some(Implementation::Musl));
    }

    #[test]
    fn ldd_musl_wins_over_glibc() {
        assert_eq!(parse_ldd("musl libc\nsome glibc mention\n"), Some(Implementation::Musl));
    }

    #[test]
    fn ldd_unknown() {
        assert_eq!(parse_ldd("some garbage\n"), None);
    }

    #[test]
    fn ldd_empty() {
        assert_eq!(parse_ldd(""), None);
    }
}
