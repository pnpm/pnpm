use pretty_assertions::assert_eq;

use super::parse_asset_name;

#[test]
fn parses_apple_silicon_zip() {
    let parsed = parse_asset_name("bun-darwin-aarch64.zip").unwrap();
    assert_eq!(parsed.platform, "darwin");
    assert_eq!(parsed.arch, "arm64");
    assert!(!parsed.musl);
}

#[test]
fn parses_linux_musl_zip() {
    let parsed = parse_asset_name("bun-linux-x64-musl.zip").unwrap();
    assert_eq!(parsed.platform, "linux");
    assert_eq!(parsed.arch, "x64");
    assert!(parsed.musl);
}

#[test]
fn parses_windows_zip() {
    let parsed = parse_asset_name("bun-windows-x64.zip").unwrap();
    assert_eq!(parsed.platform, "win32");
    assert_eq!(parsed.arch, "x64");
    assert!(!parsed.musl);
}

#[test]
fn ignores_unrelated_assets() {
    assert!(parse_asset_name("SHASUMS256.txt").is_none());
    assert!(parse_asset_name("bun-linux.zip").is_none());
    assert!(parse_asset_name("bun-linux-x64.tar.gz").is_none());
}
