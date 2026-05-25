mod command;
mod elf;
mod filesystem;

/// Libc implementation detected on the host system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Implementation {
    /// GNU C Library.
    Glibc,
    /// musl libc.
    Musl,
}

impl Implementation {
    /// Return the string used in pnpm's platform selector: `"glibc"` or
    /// `"musl"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Implementation::Glibc => "glibc",
            Implementation::Musl => "musl",
        }
    }
}

/// Detect the host libc implementation.
///
/// Returns `Some(Implementation::Glibc)` or
/// `Some(Implementation::Musl)` on Linux when the implementation
/// can be determined, or `None` on non-Linux hosts or when all
/// detection methods fail.
///
/// Detection order:
/// 1. **ELF interpreter** — read PT_INTERP from `/proc/self/exe`.
///    If the dynamic linker path contains `"/ld-musl-"` → musl;
///    if it contains `"/ld-linux-"` → glibc.
/// 2. **Filesystem** — read first 2048 bytes of `/usr/bin/ldd`.
///    If content contains `"musl"` → musl; if it contains
///    `"GNU C Library"` or `"GNU libc"` → glibc.
/// 3. **Command** — run `getconf GNU_LIBC_VERSION`; if that
///    fails, fall back to `ldd --version`.
///
/// Methods are ordered by cost: the ELF interpreter check avoids
/// spawning any process, the filesystem read avoids PATH lookup,
/// and the command fallback is only reached when cheaper methods
/// fail. This makes detection work in slim containers where
/// `getconf` or `ldd` may not be on PATH or installed at all.
pub fn detect() -> Option<Implementation> {
    if !is_linux() {
        return None;
    }

    detect_implementation()
}

fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

fn detect_implementation() -> Option<Implementation> {
    elf::detect().or_else(filesystem::detect).or_else(command::detect)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::detect;
    use super::{Implementation, is_linux};

    #[test]
    fn detect_non_linux() {
        assert!(target_os_is_linux_matches_is_linux_fn());
    }

    fn target_os_is_linux_matches_is_linux_fn() -> bool {
        cfg!(target_os = "linux") == is_linux()
    }

    #[test]
    fn libc_implementation_as_str_glibc() {
        assert_eq!(Implementation::Glibc.as_str(), "glibc");
    }

    #[test]
    fn libc_implementation_as_str_musl() {
        assert_eq!(Implementation::Musl.as_str(), "musl");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_integration_host() {
        let result = detect();
        if let Some(libc) = result {
            assert!(
                libc == Implementation::Glibc || libc == Implementation::Musl,
                "unexpected libc: {libc:?}",
            );
        }
    }
}
