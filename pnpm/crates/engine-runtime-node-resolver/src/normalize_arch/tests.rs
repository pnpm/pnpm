use pretty_assertions::assert_eq;

use super::get_normalized_arch;

#[test]
fn maps_quirky_arches_to_the_published_tarball_directory_name() {
    assert_eq!(get_normalized_arch("win32", "ia32", None), "x86");
    assert_eq!(get_normalized_arch("linux", "arm", None), "armv7l");
    assert_eq!(get_normalized_arch("linux", "x64", None), "x64");
    assert_eq!(get_normalized_arch("darwin", "arm", None), "armv7l");
}

#[test]
fn darwin_arm64_falls_back_to_x64_on_pre_node_16() {
    assert_eq!(get_normalized_arch("darwin", "arm64", Some("14.20.0")), "x64");
    assert_eq!(get_normalized_arch("darwin", "arm64", Some("16.17.0")), "arm64");
}
