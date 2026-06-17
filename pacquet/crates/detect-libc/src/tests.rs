#[cfg(target_os = "linux")]
use super::detect;
use super::{Implementation, host_arch, host_platform, is_linux};

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

#[test]
fn host_platform_uses_node_naming() {
    let platform = host_platform();
    assert!(!platform.is_empty());
    assert_ne!(platform, "macos");
    assert_ne!(platform, "windows");
    assert_ne!(platform, "solaris");
}

#[test]
fn host_arch_uses_node_naming() {
    let arch = host_arch();
    assert!(!arch.is_empty());
    assert_ne!(arch, "x86_64");
    assert_ne!(arch, "aarch64");
    assert_ne!(arch, "powerpc64le");
}
