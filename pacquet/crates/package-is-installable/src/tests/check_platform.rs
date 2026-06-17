//! Port of `config/package-is-installable/test/checkPlatform.ts`
//! at <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/test/checkPlatform.ts>.
//!
//! The upstream tests mock `process.platform` / `process.arch` /
//! `detect-libc.familySync`. Pacquet's [`check_platform`] takes the
//! three values as parameters, so the ports pass them explicitly
//! rather than mutating any global state.

use crate::{
    SupportedArchitectures, UnsupportedPlatformError, WantedPlatform, WantedPlatformRef,
    check_platform,
};

const PACKAGE_ID: &str = "registry.npmjs.org/foo/1.0.0";
const FAKE_LINUX: &str = "linux";
const FAKE_X64: &str = "x64";
const FAKE_MUSL: &str = "musl";

fn wanted(os: Option<&[&str]>, cpu: Option<&[&str]>, libc: Option<&[&str]>) -> WantedPlatform {
    fn vec_opt(values: Option<&[&str]>) -> Option<Vec<String>> {
        values.map(|slice| slice.iter().map(|item| (*item).to_string()).collect())
    }
    WantedPlatform { os: vec_opt(os), cpu: vec_opt(cpu), libc: vec_opt(libc) }
}

/// Test-local convenience wrapper. The runtime `check_platform` takes
/// the wanted axes as `Option<&[String]>` slices so the install hot
/// path doesn't have to construct a `WantedPlatform` per snapshot;
/// the tests find it more ergonomic to build one and pass it by
/// reference, so this wrapper does the `.as_deref()` for each axis
/// in one place.
fn check_platform_w(
    pkg: &str,
    wanted_platform: &WantedPlatform,
    supp: Option<&SupportedArchitectures>,
    os: &str,
    cpu: &str,
    libc: &str,
) -> Option<UnsupportedPlatformError> {
    let wanted = WantedPlatformRef {
        os: wanted_platform.os.as_deref(),
        cpu: wanted_platform.cpu.as_deref(),
        libc: wanted_platform.libc.as_deref(),
    };
    check_platform(pkg, wanted, supp, os, cpu, libc)
}

fn supported(
    os: Option<&[&str]>,
    cpu: Option<&[&str]>,
    libc: Option<&[&str]>,
) -> SupportedArchitectures {
    fn vec_opt(values: Option<&[&str]>) -> Option<Vec<String>> {
        values.map(|slice| slice.iter().map(|item| (*item).to_string()).collect())
    }
    SupportedArchitectures { os: vec_opt(os), cpu: vec_opt(cpu), libc: vec_opt(libc) }
}

#[test]
fn target_cpu_wrong() {
    let wanted_platform = wanted(Some(&["any"]), Some(&["enten-cpu"]), Some(&["any"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_some());
}

#[test]
fn os_wrong() {
    let wanted_platform = wanted(Some(&["enten-os"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_some());
}

#[test]
fn libc_wrong() {
    let wanted_platform = wanted(Some(&["any"]), Some(&["any"]), Some(&["enten-libc"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_some());
}

#[test]
fn nothing_wrong() {
    let wanted_platform = wanted(Some(&["any"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_none());
}

#[test]
fn everything_wrong_with_arrays() {
    let wanted_platform = wanted(Some(&["enten-os"]), Some(&["enten-cpu"]), Some(&["enten-libc"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_some());
}

#[test]
fn os_wrong_negation() {
    let wanted_platform = wanted(Some(&["!linux"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_some());
}

#[test]
fn nothing_wrong_negation() {
    let wanted_platform =
        wanted(Some(&["!enten-os"]), Some(&["!enten-cpu"]), Some(&["!enten-libc"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, FAKE_LINUX, FAKE_X64, FAKE_MUSL);
    assert!(err.is_none());
}

#[test]
fn override_os() {
    let supported_platform = supported(Some(&["win32"]), Some(&["current"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["win32"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn accept_another_cpu() {
    let supported_platform =
        supported(Some(&["current"]), Some(&["current", "x64"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["any"]), Some(&["x64"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        "arm64",
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn fail_when_cpu_is_different() {
    let supported_platform = supported(Some(&["current"]), Some(&["arm64"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["any"]), Some(&["x64"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_some());
}

#[test]
fn override_libc() {
    let supported_platform = supported(Some(&["current"]), Some(&["current"]), Some(&["glibc"]));
    let wanted_platform = wanted(Some(&["any"]), Some(&["any"]), Some(&["glibc"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn accept_another_libc() {
    let supported_platform =
        supported(Some(&["current"]), Some(&["current"]), Some(&["current", "glibc"]));
    let wanted_platform = wanted(Some(&["any"]), Some(&["any"]), Some(&["glibc"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn accept_negated_os_with_multi_valued_supported() {
    let supported_platform =
        supported(Some(&["linux", "current"]), Some(&["current"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["!win32"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn accept_negated_cpu_with_multi_valued_supported() {
    let supported_platform =
        supported(Some(&["current"]), Some(&["x64", "current"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["any"]), Some(&["!ia32"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        FAKE_LINUX,
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_none());
}

#[test]
fn reject_negated_os_when_any_supported_value_matches_negation() {
    let supported_platform =
        supported(Some(&["win32", "current"]), Some(&["current"]), Some(&["current"]));
    let wanted_platform = wanted(Some(&["!win32"]), Some(&["any"]), Some(&["any"]));
    let err = check_platform_w(
        PACKAGE_ID,
        &wanted_platform,
        Some(&supported_platform),
        "darwin",
        FAKE_X64,
        FAKE_MUSL,
    );
    assert!(err.is_some());
}

#[test]
fn libc_check_skipped_when_current_libc_is_unknown() {
    let wanted_platform = wanted(Some(&["any"]), Some(&["any"]), Some(&["glibc"]));
    let err = check_platform_w(PACKAGE_ID, &wanted_platform, None, "darwin", FAKE_X64, "unknown");
    assert!(err.is_none());
}
