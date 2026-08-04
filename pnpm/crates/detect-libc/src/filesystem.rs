use crate::Implementation;
use std::{fs::File, io::Read};

const LDD_PATH: &str = "/usr/bin/ldd";
const MAX_LENGTH: usize = 2048;

/// Detect libc implementation from `/usr/bin/ldd` content.
pub fn detect() -> Option<Implementation> {
    let content = read_ldd()?;
    classify_from_ldd(&content)
}

fn read_ldd() -> Option<String> {
    let mut file = File::open(LDD_PATH).ok()?;
    let mut buf = vec![0u8; MAX_LENGTH];
    let bytes_read = file.read(&mut buf).ok()?;
    buf.truncate(bytes_read);
    Some(String::from_utf8_lossy(&buf).into_owned())
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
mod tests;
