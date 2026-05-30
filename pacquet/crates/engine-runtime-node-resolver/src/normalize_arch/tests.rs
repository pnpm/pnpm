use pretty_assertions::assert_eq;

use super::get_normalized_arch;

/// Mirrors upstream's
/// [`normalizeArch.test.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/test/normalizeArch.test.ts):
/// `win32 ia32 → x86`, `linux arm → armv7l` (Raspberry Pi 4), and the
/// identity case `linux x64 → x64`.
#[test]
fn maps_quirky_arches_to_the_published_tarball_directory_name() {
    assert_eq!(get_normalized_arch("win32", "ia32", None), "x86");
    assert_eq!(get_normalized_arch("linux", "arm", None), "armv7l");
    assert_eq!(get_normalized_arch("linux", "x64", None), "x64");
    // Pacquet matches upstream's unconditional `arm → armv7l` mapping
    // — non-Linux `arm` is normalised the same way, so the assertion
    // is a regression guard against accidentally adding a platform
    // guard that diverges from pnpm.
    assert_eq!(get_normalized_arch("darwin", "arm", None), "armv7l");
}

/// Apple Silicon (`darwin arm64`) only has its own tarball from
/// Node 16. Pre-16 falls back to the `x64` Rosetta build; 16 and up
/// stay on `arm64`. Mirrors upstream's `normalizeArch.test.ts:14-19`.
#[test]
fn darwin_arm64_falls_back_to_x64_on_pre_node_16() {
    assert_eq!(get_normalized_arch("darwin", "arm64", Some("14.20.0")), "x64");
    assert_eq!(get_normalized_arch("darwin", "arm64", Some("16.17.0")), "arm64");
}
