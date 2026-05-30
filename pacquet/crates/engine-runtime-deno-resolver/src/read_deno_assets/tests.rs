use pretty_assertions::assert_eq;

use super::{extract_sha256, parse_asset_name};

/// macOS Apple Silicon — `aarch64-apple-darwin` → one `(darwin, arm64)`
/// target.
#[test]
fn parses_apple_silicon_asset_name() {
    let targets = parse_asset_name("deno-aarch64-apple-darwin.zip.sha256sum").unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].os, "darwin");
    assert_eq!(targets[0].cpu, "arm64");
}

/// Linux glibc x64 — `x86_64-unknown-linux-gnu` → one `(linux, x64)`
/// target, `libc=None` (glibc is implicit).
#[test]
fn parses_linux_glibc_asset_name() {
    let targets = parse_asset_name("deno-x86_64-unknown-linux-gnu.zip.sha256sum").unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].os, "linux");
    assert_eq!(targets[0].cpu, "x64");
    assert!(targets[0].libc.is_none());
}

/// Windows x64 covers arm64 under emulation — both targets land on
/// the same variant.
#[test]
fn windows_x64_covers_arm64_under_emulation() {
    let targets = parse_asset_name("deno-x86_64-pc-windows-msvc.zip.sha256sum").unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].os, "win32");
    assert_eq!(targets[0].cpu, "x64");
    assert_eq!(targets[1].os, "win32");
    assert_eq!(targets[1].cpu, "arm64");
}

/// Non-matching filenames fall through.
#[test]
fn ignores_unrelated_asset_names() {
    assert!(parse_asset_name("deno-linux.zip").is_none());
    assert!(parse_asset_name("deno-x86_64-unknown-freebsd.zip.sha256sum").is_none());
    assert!(parse_asset_name("README.md").is_none());
}

/// The SHA256 extractor lifts the first 64-character hex run out of an
/// arbitrary body and lower-cases it.
#[test]
fn extract_sha256_lifts_first_hex_run() {
    let body = "ED52239294AD517FBE91A268146D5D2AA8A17D2D62D64873E43219078BA71C4E  deno.zip";
    let hash = extract_sha256(body).unwrap();
    assert_eq!(hash, "ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e");
}

/// No 64-char hex run → `None`.
#[test]
fn extract_sha256_none_when_missing() {
    assert!(extract_sha256("no hashes here").is_none());
}
