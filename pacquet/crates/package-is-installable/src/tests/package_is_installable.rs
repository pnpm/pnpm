//! End-to-end tests for [`crate::package_is_installable`], exercising
//! the tri-state verdict and the optional / engine-strict branches.
//!
//! Upstream has no direct integration tests for this composer
//! (`packageIsInstallable` in `index.ts` is exercised end-to-end via
//! `installing/deps-installer/test/install/optionalDependencies.ts`).
//! These pacquet tests pin the contract at the function boundary so
//! callers can rely on it without spinning up the full install
//! pipeline.

use crate::{
    InstallabilityOptions, InstallabilityVerdict, PackageInstallabilityManifest, SkipReason,
    WantedEngine, package_is_installable,
};

fn host_linux_x64() -> InstallabilityOptions<'static> {
    InstallabilityOptions {
        engine_strict: false,
        optional: false,
        current_node_version: "20.10.0",
        pnpm_version: None,
        current_os: "linux",
        current_cpu: "x64",
        current_libc: "glibc",
        supported_architectures: None,
    }
}

#[test]
fn compatible_manifest_is_installable() {
    let manifest = PackageInstallabilityManifest::default();
    let verdict = package_is_installable("pkg", &manifest, &host_linux_x64()).unwrap();
    assert_eq!(verdict, InstallabilityVerdict::Installable);
}

#[test]
fn incompatible_optional_is_skipped_with_platform_reason() {
    let manifest = PackageInstallabilityManifest {
        os: Some(vec!["this-os-does-not-exist".to_string()]),
        ..Default::default()
    };
    let mut opts = host_linux_x64();
    opts.optional = true;

    let verdict = package_is_installable("pkg", &manifest, &opts).unwrap();
    match verdict {
        InstallabilityVerdict::SkipOptional { reason, .. } => {
            assert_eq!(reason, SkipReason::UnsupportedPlatform);
        }
        other => panic!("expected SkipOptional, got {other:?}"),
    }
}

#[test]
fn incompatible_optional_engine_is_skipped_with_engine_reason() {
    let manifest = PackageInstallabilityManifest {
        engines: Some(WantedEngine { node: Some("0.10".to_string()), ..Default::default() }),
        ..Default::default()
    };
    let mut opts = host_linux_x64();
    opts.optional = true;

    let verdict = package_is_installable("pkg", &manifest, &opts).unwrap();
    match verdict {
        InstallabilityVerdict::SkipOptional { reason, .. } => {
            assert_eq!(reason, SkipReason::UnsupportedEngine);
        }
        other => panic!("expected SkipOptional, got {other:?}"),
    }
}

#[test]
fn incompatible_non_optional_proceeds_with_warning() {
    let manifest = PackageInstallabilityManifest {
        os: Some(vec!["this-os-does-not-exist".to_string()]),
        ..Default::default()
    };

    let verdict = package_is_installable("pkg", &manifest, &host_linux_x64()).unwrap();
    match verdict {
        InstallabilityVerdict::ProceedWithWarning { .. } => {}
        other => panic!("expected ProceedWithWarning, got {other:?}"),
    }
}

#[test]
fn incompatible_non_optional_strict_returns_error() {
    let manifest = PackageInstallabilityManifest {
        os: Some(vec!["this-os-does-not-exist".to_string()]),
        ..Default::default()
    };
    let mut opts = host_linux_x64();
    opts.engine_strict = true;

    let err = package_is_installable("pkg", &manifest, &opts).expect_err("strict must error");
    assert_eq!(err.skip_reason(), SkipReason::UnsupportedPlatform);
}

#[test]
fn platform_is_evaluated_before_engine() {
    // A manifest that fails both platform and engine surfaces the
    // platform error first — mirrors upstream `checkPackage`'s
    // `checkPlatform ?? checkEngine` short-circuit.
    let manifest = PackageInstallabilityManifest {
        os: Some(vec!["this-os-does-not-exist".to_string()]),
        engines: Some(WantedEngine { node: Some("0.10".to_string()), ..Default::default() }),
        ..Default::default()
    };
    let mut opts = host_linux_x64();
    opts.optional = true;

    let verdict = package_is_installable("pkg", &manifest, &opts).unwrap();
    match verdict {
        InstallabilityVerdict::SkipOptional { reason, .. } => {
            assert_eq!(reason, SkipReason::UnsupportedPlatform);
        }
        other => panic!("expected SkipOptional(UnsupportedPlatform), got {other:?}"),
    }
}
