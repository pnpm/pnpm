//! Embeds the Windows version resource in `pnpm.exe`.
//!
//! The code signing certificate requires every signed binary to carry the
//! product name and version.

use std::{env, fs};

const DEFAULTS_RS: &str = "../config/src/defaults.rs";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={DEFAULTS_RS}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Only a Windows host has the resource compiler. Checking a Windows
    // target from another host, as dylint does, builds no release binary.
    if !cfg!(windows) {
        return;
    }
    let version = read_pnpm_version();
    let mut resource = winresource::WindowsResource::new();
    resource
        .set("ProductName", "pnpm")
        .set("FileDescription", "pnpm")
        .set("InternalName", "pnpm")
        .set("OriginalFilename", "pnpm.exe")
        .set("ProductVersion", &version)
        .set("FileVersion", &version)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, encode_version(&version))
        .set_version_info(winresource::VersionInfo::FILEVERSION, encode_version(&version));
    resource.compile().expect("compile the Windows version resource");
}

/// Reads `PNPM_VERSION` from the config crate, the one place the version is
/// committed. The release workflow verifies it against the tag.
fn read_pnpm_version() -> String {
    const PREFIX: &str = r#"pub const PNPM_VERSION: &str = ""#;
    let source = fs::read_to_string(DEFAULTS_RS).expect("read pnpm-config's defaults.rs");
    let start = source.find(PREFIX).expect("find PNPM_VERSION in defaults.rs") + PREFIX.len();
    let length = source[start..].find('"').expect("find the end of PNPM_VERSION");
    source[start..start + length].to_string()
}

/// Packs `MAJOR.MINOR.PATCH` into the four 16-bit words of a `VS_FIXEDFILEINFO`
/// version. A prerelease suffix is dropped: the numeric form has no place for it.
fn encode_version(version: &str) -> u64 {
    let release = version
        .split(['-', '+'])
        .next()
        .unwrap_or(version);
    release
        .split('.')
        .map(|part| part.parse::<u64>().expect("numeric version component"))
        .chain(std::iter::repeat(0))
        .take(4)
        .fold(0, |packed, part| (packed << 16) | part)
}
