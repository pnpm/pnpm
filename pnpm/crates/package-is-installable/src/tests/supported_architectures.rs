//! `supportedArchitectures` written as a list of platforms: how each
//! entry is read, which platforms the axes stand for, and which packages
//! a named platform takes.

use crate::{
    ArchitectureAxes, SupportedArchitectures, SupportedPlatform, WantedPlatformRef,
    platform_is_supported,
};
use pretty_assertions::assert_eq;

/// The host the axes fall back on, and the one `current` names.
const HOST: (&str, &str, &str) = ("linux", "x64", "glibc");

fn spelled(entry: &str) -> String {
    entry
        .parse::<SupportedPlatform>()
        .unwrap_or_else(|error| panic!("{entry}: {error}"))
        .to_string()
}

/// A lockfile resolved for one spelling still answers for the other.
#[test]
fn a_platform_reads_the_same_in_either_spelling() {
    for (triple, platform) in [
        ("x86_64-unknown-linux-gnu", "linux-x64"),
        ("x86_64-unknown-linux-musl", "linux-x64-musl"),
        ("aarch64-apple-darwin", "darwin-arm64"),
        ("x86_64-pc-windows-msvc", "win32-x64"),
        ("i686-pc-windows-msvc", "win32-ia32"),
        ("powerpc64-unknown-linux-gnu", "linux-ppc64be"),
        ("powerpc64le-unknown-linux-gnu", "linux-ppc64le"),
        ("riscv64gc-unknown-linux-musl", "linux-riscv64-musl"),
    ] {
        assert_eq!(spelled(triple), platform, "{triple}");
        assert_eq!(spelled(platform), platform);
    }
}

/// `linux-x64` means the glibc build everywhere else in pnpm.
#[test]
fn a_linux_platform_defaults_to_glibc() {
    assert_eq!(spelled("linux-x64"), "linux-x64");
    assert_eq!(spelled("linux-x64-gnu"), "linux-x64");
    assert_eq!(spelled("linux-x64-glibc"), "linux-x64");
}

/// The family alone does not say which glibc or musl releases a wheel may
/// be built against, so a baseline cannot be normalized down to one.
#[test]
fn a_libc_baseline_survives_the_spelling() {
    assert_eq!(spelled("linux-x64-manylinux_2_28"), "linux-x64-manylinux_2_28");
    assert_eq!(spelled("x86_64-manylinux_2_28"), "linux-x64-manylinux_2_28");
    assert_eq!(spelled("linux-arm64-musllinux_1_1"), "linux-arm64-musllinux_1_1");
}

/// `ppc64` is the one name Node has for either endianness.
#[test]
fn both_powerpc_platforms_are_one_name_to_a_package() {
    let platforms = listed(&["linux-ppc64le", "linux-ppc64be"]);
    assert!(takes(&platforms, "linux", "ppc64", "glibc"));
    assert_eq!(
        platforms
            .platforms(HOST.0, HOST.1, HOST.2)
            .iter()
            .map(|platform| platform.architecture.wheel())
            .collect::<Vec<_>>(),
        ["ppc64le", "ppc64"],
    );
}

/// `ppc64` is the one name Node has for either endianness, and wheels
/// are published for the little-endian one, so that is what a package's
/// own name reads as.
#[test]
fn a_bare_ppc64_is_the_little_endian_platform() {
    assert_eq!(spelled("linux-ppc64"), "linux-ppc64le");
    assert_eq!(crossed(&["linux"], &["ppc64"], &[]), ["linux-ppc64le"]);
}

/// The running platform is read from the architecture spelling that
/// tells the two POWER endiannesses apart. No machine pnpm builds on is
/// one, so this equality is all that holds the callers to it there.
#[test]
fn the_host_is_read_from_the_architecture_that_names_it_exactly() {
    let host = SupportedArchitectures::Platforms(vec!["current".parse().unwrap()]);
    assert_eq!(
        host.host_platforms(),
        host.platforms(
            pnpm_detect_libc::host_platform(),
            std::env::consts::ARCH,
            pnpm_detect_libc::detect().map_or("unknown", |libc| libc.as_str()),
        ),
    );
}

/// A POWER host keeps the endianness it is, which the Node name cannot
/// carry.
#[test]
fn current_keeps_the_endianness_the_host_is() {
    let host = SupportedArchitectures::Platforms(vec!["current".parse().unwrap()]);
    let wheel = |architecture| {
        host.platforms("linux", architecture, "glibc")
            .iter()
            .map(|platform| platform.architecture.wheel())
            .collect::<Vec<_>>()
    };
    assert_eq!(wheel("powerpc64le"), ["ppc64le"]);
    assert_eq!(wheel("powerpc64"), ["ppc64"]);
}

/// A baseline carries the two numbers of a libc release. Which releases
/// exist is the interpreter's to say; that a value is not a baseline at
/// all is a configuration error, and reads as one here.
#[test]
fn refuses_a_wheel_baseline_that_names_no_release() {
    for entry in [
        "linux-x64-manylinux",
        "linux-x64-manylinux_2",
        "linux-x64-manylinux_bad",
        "linux-x64-musllinux_1_bad",
        "linux-x64-musllinux__2",
    ] {
        entry.parse::<SupportedPlatform>().expect_err(entry);
    }
    assert_eq!(spelled("linux-x64-manylinux_2_28"), "linux-x64-manylinux_2_28");
}

/// The running platform is one platform however it is reached, so a host
/// whose C library pnpm cannot detect still reads as the platform an
/// explicit `linux-x64` names.
#[test]
fn current_is_one_platform_with_the_name_it_also_has() {
    let listed = listed(&["current", "linux-x64"]);
    assert_eq!(listed.platforms("linux", "x64", "unknown").len(), 1);
    assert_eq!(named(&listed), ["linux-x64"]);
}

#[test]
fn refuses_an_entry_that_does_not_name_a_platform() {
    for entry in ["x86_64-linux", "linux", "linux-enten", "enten-x64", "linux-x64-enten"] {
        let error = entry.parse::<SupportedPlatform>().expect_err(entry);
        assert_eq!(format!("{error}"), format!("pnpm does not know the platform {entry}"));
    }
}

/// A C library belongs to a Linux platform, so naming one anywhere else
/// is a mistake worth its own message.
#[test]
fn refuses_a_c_library_on_a_platform_that_has_none() {
    let error = "darwin-arm64-musl".parse::<SupportedPlatform>().expect_err("a C library on macOS");
    assert_eq!(
        format!("{error}"),
        "only a Linux platform names a C library, and darwin-arm64-musl is not one",
    );
}

fn axis(values: &[&str]) -> Option<Vec<String>> {
    if values.is_empty() {
        return None;
    }
    Some(
        values
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    )
}

fn crossed(os: &[&str], cpu: &[&str], libc: &[&str]) -> Vec<String> {
    named(&SupportedArchitectures::Axes(ArchitectureAxes {
        os: axis(os),
        cpu: axis(cpu),
        libc: axis(libc),
    }))
}

fn listed(entries: &[&str]) -> SupportedArchitectures {
    SupportedArchitectures::Platforms(
        entries
            .iter()
            .map(|entry| {
                entry
                    .parse()
                    .unwrap_or_else(|error| panic!("{entry}: {error}"))
            })
            .collect(),
    )
}

fn named(supported: &SupportedArchitectures) -> Vec<String> {
    supported
        .platforms(HOST.0, HOST.1, HOST.2)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// A cross product is what an `os` and a `cpu` list have always meant
/// to the optional-dependency check.
#[test]
fn the_axes_stand_for_every_platform_they_cross_into() {
    assert_eq!(
        crossed(&["linux", "darwin"], &["x64", "arm64"], &[]),
        ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"],
    );
}

/// Only Linux has a C library.
#[test]
fn a_libc_axis_only_reaches_linux() {
    assert_eq!(
        crossed(&["linux", "win32"], &["x64"], &["glibc", "musl"]),
        ["linux-x64", "linux-x64-musl", "win32-x64"],
    );
}

/// The axes are read by the optional-dependency check too, where any
/// name a package may declare is meaningful, so one that names no
/// platform is left out rather than reported.
#[test]
fn an_axis_value_that_names_no_platform_is_left_out() {
    assert_eq!(crossed(&["linux", "freebsd"], &["x64", "sparc"], &[]), ["linux-x64"]);
}

#[test]
fn current_is_the_platform_the_install_runs_on() {
    assert_eq!(crossed(&["current"], &["arm64"], &[]), ["linux-arm64"]);
    assert_eq!(crossed(&[], &["arm64"], &[]), ["linux-arm64"]);
    assert_eq!(named(&listed(&["current", "darwin-arm64"])), ["linux-x64", "darwin-arm64"]);
}

#[test]
fn a_platform_list_names_the_platforms_themselves() {
    assert_eq!(
        named(&listed(&["linux-x64-manylinux_2_28", "darwin-arm64", "win32-x64"])),
        ["linux-x64-manylinux_2_28", "darwin-arm64", "win32-x64"],
    );
}

#[test]
fn a_platform_named_twice_is_one_platform() {
    assert_eq!(named(&listed(&["linux-x64", "x86_64-unknown-linux-gnu"])), ["linux-x64"]);
}

fn unnamed(supported: &SupportedArchitectures) -> Vec<&str> {
    supported.unnamed_platform_values(HOST.0, HOST.1, HOST.2)
}

/// A caller that prepares per platform has to tell the user which of the
/// names it could not prepare for, since `platforms` leaves them out
/// without a word.
#[test]
fn names_the_axis_values_no_platform_stands_for() {
    assert_eq!(
        unnamed(&SupportedArchitectures::Axes(ArchitectureAxes {
            os: axis(&["linux", "freebsd"]),
            cpu: axis(&["x64", "sparc"]),
            libc: axis(&["musl", "uclibc"]),
        })),
        ["freebsd", "sparc", "uclibc"],
    );
}

#[test]
fn names_nothing_when_every_value_stands_for_a_platform() {
    let axes = SupportedArchitectures::Axes(ArchitectureAxes {
        os: axis(&["linux", "current"]),
        cpu: axis(&["x64"]),
        libc: axis(&["current", "manylinux_2_28"]),
    });
    assert!(unnamed(&axes).is_empty());
    assert!(unnamed(&listed(&["linux-x64", "current"])).is_empty());
}

fn takes(supported: &SupportedArchitectures, os: &str, cpu: &str, libc: &str) -> bool {
    let (os, cpu, libc) = ([os.to_string()], [cpu.to_string()], [libc.to_string()]);
    platform_is_supported(
        WantedPlatformRef { os: Some(&os), cpu: Some(&cpu), libc: Some(&libc) },
        Some(supported),
        HOST.0,
        HOST.1,
        HOST.2,
    )
}

/// The axes take a combination no listed platform is, because each of
/// them matches on its own.
#[test]
fn a_platform_list_refuses_a_combination_no_platform_of_it_is() {
    let platforms = listed(&["linux-x64", "darwin-arm64"]);
    assert!(takes(&platforms, "linux", "x64", "glibc"));
    assert!(takes(&platforms, "darwin", "arm64", "glibc"));
    assert!(!takes(&platforms, "darwin", "x64", "glibc"));
    assert!(!takes(&platforms, "linux", "arm64", "glibc"));

    let crossed = SupportedArchitectures::Axes(ArchitectureAxes {
        os: Some(vec!["linux".to_string(), "darwin".to_string()]),
        cpu: Some(vec!["x64".to_string(), "arm64".to_string()]),
        libc: None,
    });
    assert!(takes(&crossed, "darwin", "x64", "glibc"));
}

/// A platform that has no C library leaves the axis to the package.
#[test]
fn a_named_platform_answers_for_its_own_c_library() {
    let platforms = listed(&["linux-x64-musl", "win32-x64"]);
    assert!(takes(&platforms, "linux", "x64", "musl"));
    assert!(!takes(&platforms, "linux", "x64", "glibc"));
    assert!(takes(&platforms, "win32", "x64", "glibc"));
}
