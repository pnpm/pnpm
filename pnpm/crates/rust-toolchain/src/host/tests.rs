use super::windows_abi;
use pretty_assertions::assert_eq;

#[test]
fn a_mingw_build_installs_the_mingw_toolchain() {
    assert_eq!(windows_abi(true, false), "gnu");
}

#[test]
fn an_llvm_mingw_build_installs_the_llvm_mingw_toolchain() {
    assert_eq!(windows_abi(true, true), "gnullvm");
}

#[test]
fn any_other_build_installs_the_msvc_toolchain() {
    assert_eq!(windows_abi(false, false), "msvc");
}

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn an_msvc_build_installs_the_msvc_toolchain() {
    assert!(super::host_triple().unwrap().ends_with("-pc-windows-msvc"));
}
