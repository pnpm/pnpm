use std::fs::File;
use std::io::Read;

use crate::Implementation;

const LDD_PATH: &str = "/usr/bin/ldd";
const MAX_LENGTH: usize = 2048;

/// Detect libc implementation from `/usr/bin/ldd` content.
///
/// Reads the first 2048 bytes and classifies based on known
/// strings: `"musl"` wins over `"GNU C Library"` / `"GNU libc"`.
pub fn detect() -> Option<Implementation> {
    let content = read_ldd()?;
    classify_from_ldd(&content)
}

fn read_ldd() -> Option<String> {
    let mut file = File::open(LDD_PATH).ok()?;
    let mut buf = vec![0u8; MAX_LENGTH];
    let bytes_read = file.read(&mut buf).ok()?;
    buf.truncate(bytes_read);
    String::from_utf8(buf).ok()
}

fn classify_from_ldd(content: &str) -> Option<Implementation> {
    if content.contains("musl") {
        return Some(Implementation::Musl);
    }
    if content.contains("GNU C Library") || content.contains("GNU libc") {
        return Some(Implementation::Glibc);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::classify_from_ldd;
    use crate::Implementation;

    #[test]
    fn glibc_detected_via_gnu_c_library() {
        assert_eq!(
            classify_from_ldd("#!/bin/bash\n# GNU C Library (glibc) ldd script\n"),
            Some(Implementation::Glibc),
        );
    }

    #[test]
    fn glibc_detected_via_gnu_libc() {
        assert_eq!(classify_from_ldd("ldd (GNU libc) 2.39\n"), Some(Implementation::Glibc),);
    }

    #[test]
    fn musl_detected_via_ldd() {
        assert_eq!(classify_from_ldd("musl libc\nVersion 1.2.3\n"), Some(Implementation::Musl),);
    }

    #[test]
    fn unknown_ldd_content_returns_none() {
        assert_eq!(classify_from_ldd("not a libc at all\n"), None);
    }

    #[test]
    fn empty_ldd_content_returns_none() {
        assert_eq!(classify_from_ldd(""), None);
    }
}
