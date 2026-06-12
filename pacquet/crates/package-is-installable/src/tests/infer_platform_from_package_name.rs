//! Port of `config/package-is-installable/test/inferPlatformFromPackageName.ts`
//! from
//! <https://github.com/pnpm/pnpm/blob/34875b2d7c/config/package-is-installable/test/inferPlatformFromPackageName.ts>.

use crate::{
    InstallabilityOptions, InstallabilityVerdict, PackageInstallabilityManifest,
    SupportedArchitectures, WantedPlatform, infer_platform_from_package_name,
    package_is_installable,
};
use pretty_assertions::assert_eq;

fn platform(
    os: Option<&[&str]>,
    cpu: Option<&[&str]>,
    libc: Option<&[&str]>,
) -> Option<WantedPlatform> {
    Some(WantedPlatform { os: owned(os), cpu: owned(cpu), libc: owned(libc) })
}

fn owned(values: Option<&[&str]>) -> Option<Vec<String>> {
    values.map(|values| values.iter().map(|value| (*value).to_string()).collect())
}

#[test]
fn infers_platform_from_real_world_names() {
    let cases: &[(&str, Option<WantedPlatform>)] = &[
        ("@nx/nx-win32-arm64-msvc", platform(Some(&["win32"]), Some(&["arm64"]), None)),
        (
            "@nx/nx-linux-arm-gnueabihf",
            platform(Some(&["linux"]), Some(&["arm"]), Some(&["glibc"])),
        ),
        ("@nx/nx-linux-x64-gnu", platform(Some(&["linux"]), Some(&["x64"]), Some(&["glibc"]))),
        ("@esbuild/aix-ppc64", platform(Some(&["aix"]), Some(&["ppc64"]), None)),
        ("@esbuild/openharmony-arm64", platform(Some(&["openharmony"]), Some(&["arm64"]), None)),
        (
            "@biomejs/cli-linux-x64-musl",
            platform(Some(&["linux"]), Some(&["x64"]), Some(&["musl"])),
        ),
        (
            "@typescript/native-preview-darwin-arm64",
            platform(Some(&["darwin"]), Some(&["arm64"]), None),
        ),
        ("turbo-windows-64", platform(Some(&["win32"]), None, None)),
        ("esbuild-darwin-64", platform(Some(&["darwin"]), None, None)),
        ("bun-linux-aarch64", platform(Some(&["linux"]), Some(&["arm64"]), None)),
        ("sharp-linux-armv7", platform(Some(&["linux"]), Some(&["arm"]), None)),
        ("is-arm", platform(None, Some(&["arm"]), None)),
        ("fsevents", None),
        ("lodash", None),
        ("@pnpm.e2e/not-compatible-with-any-os", None),
    ];
    for (name, expected) in cases {
        assert_eq!(&infer_platform_from_package_name(name), expected, "name: {name}");
    }
}

fn optional_on_linux_x64(supported: Option<&SupportedArchitectures>) -> InstallabilityOptions<'_> {
    InstallabilityOptions {
        engine_strict: false,
        optional: true,
        current_node_version: "20.10.0",
        pnpm_version: None,
        current_os: "linux",
        current_cpu: "x64",
        current_libc: "glibc",
        supported_architectures: supported,
    }
}

fn supported_linux_x64_glibc() -> SupportedArchitectures {
    SupportedArchitectures {
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        libc: Some(vec!["glibc".to_string()]),
    }
}

#[test]
fn optional_dependency_without_platform_fields_is_skipped_by_name() {
    let manifest = PackageInstallabilityManifest {
        name: "@nx/nx-win32-arm64-msvc".to_string(),
        ..Default::default()
    };
    let verdict = package_is_installable(
        "@nx/nx-win32-arm64-msvc@1.0.0",
        &manifest,
        &optional_on_linux_x64(None),
    )
    .unwrap();
    assert!(matches!(verdict, InstallabilityVerdict::SkipOptional { .. }), "got {verdict:?}");
}

#[test]
fn missing_libc_is_taken_from_the_name_when_other_fields_are_declared() {
    let supported = supported_linux_x64_glibc();
    let options = optional_on_linux_x64(Some(&supported));
    let musl = PackageInstallabilityManifest {
        name: "@nx/nx-linux-x64-musl".to_string(),
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        ..Default::default()
    };
    let verdict = package_is_installable("@nx/nx-linux-x64-musl@1.0.0", &musl, &options).unwrap();
    assert!(matches!(verdict, InstallabilityVerdict::SkipOptional { .. }), "got {verdict:?}");

    let gnu = PackageInstallabilityManifest {
        name: "@nx/nx-linux-x64-gnu".to_string(),
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        ..Default::default()
    };
    let verdict = package_is_installable("@nx/nx-linux-x64-gnu@1.0.0", &gnu, &options).unwrap();
    assert_eq!(verdict, InstallabilityVerdict::Installable);
}

#[test]
fn missing_cpu_is_taken_from_the_name_of_a_package_that_declares_its_platform() {
    let manifest = PackageInstallabilityManifest {
        name: "@pnpm.e2e/some-pkg-arm64".to_string(),
        os: Some(vec!["linux".to_string()]),
        ..Default::default()
    };
    let verdict = package_is_installable(
        "@pnpm.e2e/some-pkg-arm64@1.0.0",
        &manifest,
        &optional_on_linux_x64(None),
    )
    .unwrap();
    assert!(matches!(verdict, InstallabilityVerdict::SkipOptional { .. }), "got {verdict:?}");
}

#[test]
fn declared_platform_fields_take_precedence_over_the_name() {
    let manifest = PackageInstallabilityManifest {
        name: "@pnpm.e2e/win32-binary".to_string(),
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        libc: Some(vec!["glibc".to_string()]),
        ..Default::default()
    };
    let supported = supported_linux_x64_glibc();
    let verdict = package_is_installable(
        "@pnpm.e2e/win32-binary@1.0.0",
        &manifest,
        &optional_on_linux_x64(Some(&supported)),
    )
    .unwrap();
    assert_eq!(verdict, InstallabilityVerdict::Installable);
}

#[test]
fn package_without_declared_fields_is_not_skipped_without_an_os_token() {
    let manifest =
        PackageInstallabilityManifest { name: "is-arm".to_string(), ..Default::default() };
    let verdict =
        package_is_installable("is-arm@1.0.0", &manifest, &optional_on_linux_x64(None)).unwrap();
    assert_eq!(verdict, InstallabilityVerdict::Installable);
}

#[test]
fn platform_is_not_inferred_for_a_non_optional_dependency() {
    let manifest = PackageInstallabilityManifest {
        name: "@nx/nx-win32-arm64-msvc".to_string(),
        ..Default::default()
    };
    let mut options = optional_on_linux_x64(None);
    options.optional = false;
    let verdict =
        package_is_installable("@nx/nx-win32-arm64-msvc@1.0.0", &manifest, &options).unwrap();
    assert_eq!(verdict, InstallabilityVerdict::Installable);
}
