//! `supportedArchitectures` entries written as platform tags, the
//! vocabulary pnpr and package variants share: the floors and ABI they
//! state, the systems beyond Linux, macOS, and Windows they name, and the
//! spellings pnpm read before tags, which still name the same platform.

use crate::{NamedPlatform, SupportedPlatform};
use pnpm_platform_variants::{Floor, Libc, NodeAbi, Platform};
use pretty_assertions::assert_eq;

fn named(entry: &str) -> NamedPlatform {
    match entry.parse::<SupportedPlatform>() {
        Ok(SupportedPlatform::Named(named)) => named,
        other => panic!("{entry}: {other:?}"),
    }
}

fn tag(os: &str, cpu: &str) -> Platform {
    Platform { os: Some(os.into()), cpu: Some(cpu.into()), ..Platform::default() }
}

fn linux(cpu: &str) -> Platform {
    Platform { libc: Some(Libc::Glibc), ..tag("linux", cpu) }
}

#[test]
fn a_tag_states_its_floors_and_abi() {
    let floor = |major, minor| Some(Floor { major, minor });
    for (entry, platform) in [
        ("freebsd-x64", tag("freebsd", "x64")),
        ("linux-x64-manylinux_2_28", Platform { libc_version: floor(2, 28), ..linux("x64") }),
        (
            "darwin-arm64-macos_14_0",
            Platform { os_version: floor(14, 0), ..tag("darwin", "arm64") },
        ),
        ("linux-x64-napi8", Platform { napi: Some(8), ..linux("x64") }),
        ("linux-x64-node127", Platform { node_abi: Some(NodeAbi::Node(127)), ..linux("x64") }),
        ("linux-arm64-musl", Platform { libc: Some(Libc::Musl), ..tag("linux", "arm64") }),
        ("win32-x64-windows_10_0", Platform { os_version: floor(10, 0), ..tag("win32", "x64") }),
    ] {
        assert_eq!(named(entry).platform, platform, "{entry}");
        assert_eq!(named(entry).to_string(), entry);
    }
}

/// The spellings pnpm read before tags name the platform their tag names,
/// and keep the name they had.
#[test]
fn an_older_spelling_names_the_platform_its_tag_names() {
    for (entry, canonical, spelled) in [
        ("linux-amd64", "linux-x64", "linux-x64"),
        ("linux-x86_64-glibc", "linux-x64", "linux-x64"),
        ("linux-x64-gnu", "linux-x64", "linux-x64"),
        ("x86_64-unknown-linux-musl", "linux-x64-musl", "linux-x64-musl"),
        ("aarch64-apple-darwin", "darwin-arm64", "darwin-arm64"),
        ("x86_64-manylinux_2_28", "linux-x64-manylinux_2_28", "linux-x64-manylinux_2_28"),
        ("linux-arm64-musllinux_1_1", "linux-arm64-musl", "linux-arm64-musllinux_1_1"),
        ("powerpc64-unknown-linux-gnu", "linux-ppc64", "linux-ppc64be"),
    ] {
        let platform = named(entry);
        assert_eq!(platform.platform, named(canonical).platform, "{entry}");
        assert_eq!(platform.to_string(), spelled, "{entry}");
    }
    assert_eq!(
        named("linux-arm64-musllinux_1_1").wheel_baseline().as_deref(),
        Some("musllinux_1_1"),
    );
    assert_eq!(
        named("linux-x64-manylinux_2_28").wheel_baseline().as_deref(),
        Some("manylinux_2_28"),
    );
}

/// A system pnpm has no Python wheels for is a platform all the same,
/// for the packages that declare it.
#[test]
fn a_system_without_python_wheels_has_no_machine() {
    assert_eq!(named("freebsd-x64").machine(), None);
    assert_eq!(named("linux-loong64").machine(), None);
    assert!(named("linux-x64").machine().is_some());
}

#[test]
fn refuses_a_tag_that_names_no_platform() {
    for entry in [
        "wasm32",
        "linux-wasm32",
        "freebsd-enten",
        "enten-x64",
        "linux-x64-napi",
        "linux-x64-macos_14_0",
        "linux-x64-musl-manylinux_2_28",
        "linux-x64-napi8-musl",
        "linux-x64-napi8-node127",
    ] {
        let error = entry.parse::<SupportedPlatform>().expect_err(entry);
        assert_eq!(format!("{error}"), format!("pnpm does not know the platform {entry}"));
    }
    let error =
        "freebsd-x64-musl".parse::<SupportedPlatform>().expect_err("a C library on FreeBSD");
    assert_eq!(
        format!("{error}"),
        "only a Linux platform names a C library, and freebsd-x64-musl is not one",
    );
}

#[test]
fn a_listed_tag_takes_the_packages_of_its_system() {
    let listed = crate::SupportedArchitectures::Platforms(vec![
        "freebsd-x64".parse().unwrap(),
        "linux-arm64-manylinux_2_28".parse().unwrap(),
    ]);
    let takes = |os: &str, cpu: &str, libc: &str| {
        let (os, cpu, libc) = ([os.to_string()], [cpu.to_string()], [libc.to_string()]);
        crate::platform_is_supported(
            crate::WantedPlatformRef { os: Some(&os), cpu: Some(&cpu), libc: Some(&libc) },
            Some(&listed),
            "linux",
            "x64",
            "glibc",
        )
    };
    assert!(takes("freebsd", "x64", "glibc"));
    assert!(takes("linux", "arm64", "glibc"));
    assert!(!takes("linux", "x64", "glibc"));
    assert!(!takes("linux", "arm64", "musl"));
}

#[test]
fn an_entry_naming_two_c_libraries_is_refused() {
    for entry in ["linux-x64-glibc-musl", "linux-x64-musl-gnu", "linux-x64-musl-manylinux_2_28"] {
        let parsed = entry.parse::<SupportedPlatform>();
        assert!(parsed.is_err(), "{entry} parsed as {parsed:?}");
    }
}
