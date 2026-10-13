use super::{built_windows_abi, rustup_default_host_abi};
use pretty_assertions::assert_eq;

#[test]
fn rustup_default_host_chooses_the_abi() {
    assert_eq!(rustup_default_host_abi(Some("x86_64-pc-windows-gnu")), Some("gnu"));
    assert_eq!(rustup_default_host_abi(Some("aarch64-pc-windows-gnullvm")), Some("gnullvm"));
    assert_eq!(rustup_default_host_abi(Some("i686-pc-windows-msvc")), Some("msvc"));
}

#[test]
fn a_non_windows_rustup_default_host_does_not_choose_the_abi() {
    assert_eq!(rustup_default_host_abi(Some("x86_64-unknown-linux-gnu")), None);
    assert_eq!(rustup_default_host_abi(None), None);
}

#[test]
fn a_mingw_build_installs_the_mingw_toolchain() {
    assert_eq!(built_windows_abi(true, false), "gnu");
}

#[test]
fn an_llvm_mingw_build_installs_the_llvm_mingw_toolchain() {
    assert_eq!(built_windows_abi(true, true), "gnullvm");
}

#[test]
fn any_other_build_installs_the_msvc_toolchain() {
    assert_eq!(built_windows_abi(false, false), "msvc");
}
