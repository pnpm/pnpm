//! A package published with platform variants and no archive that targets
//! `any` installs only where a variant does.

use super::{host, no_importers, snapshot_key, synthetic_metadata};
use crate::installability::{InstallabilityHost, SkippedSnapshots, compute_skipped_snapshots};
use pnpm_lockfile::{
    LockfileResolution, PlatformAssetResolution, PlatformAssetTarget, RegistryResolution,
    SnapshotEntry, VariationsResolution,
};
use pnpm_reporter::SilentReporter;
use std::collections::HashMap;

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn variant(target: PlatformAssetTarget) -> PlatformAssetResolution {
    PlatformAssetResolution {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: INTEGRITY.parse().unwrap(),
            revision: None,
        }),
        targets: vec![target],
    }
}

fn target(os: &str, cpu: &str) -> PlatformAssetTarget {
    PlatformAssetTarget { os: os.into(), cpu: cpu.into(), ..PlatformAssetTarget::default() }
}

/// Whether an optional `example-cli@1.0.0` with `variants` is skipped on a
/// host of `os` and `cpu`.
fn skipped_on(
    variants: Vec<PlatformAssetResolution>,
    (os, cpu): (&'static str, &'static str),
) -> bool {
    let optional = SnapshotEntry { optional: true, ..Default::default() };
    skipped_as(optional, variants, &host("20.10.0", os, cpu))
}

/// Whether `example-cli@1.0.0`, as `snapshot`, with `variants` is skipped on
/// `installability_host`.
fn skipped_as(
    snapshot: SnapshotEntry,
    variants: Vec<PlatformAssetResolution>,
    installability_host: &InstallabilityHost,
) -> bool {
    let key = snapshot_key("example-cli@1.0.0");
    let snapshots = HashMap::from([(key.clone(), snapshot)]);
    let mut metadata = synthetic_metadata(None, None, None, None);
    metadata.resolution = LockfileResolution::Variations(VariationsResolution::new(variants));
    let packages = HashMap::from([(key.clone(), metadata)]);
    compute_skipped_snapshots::<SilentReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        installability_host,
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap()
    .contains(&key)
}

#[test]
fn an_optional_package_none_of_whose_variants_fits_is_skipped() {
    let linux = || vec![variant(target("linux", "x64"))];
    assert!(skipped_on(linux(), ("darwin", "arm64")));
    assert!(!skipped_on(linux(), ("linux", "x64")));
}

#[test]
fn an_any_variant_or_a_wasm32_variant_fits_every_host() {
    let any = PlatformAssetResolution::any(LockfileResolution::Registry(RegistryResolution {
        integrity: INTEGRITY.parse().unwrap(),
        revision: None,
    }));
    assert!(!skipped_on(vec![any, variant(target("linux", "x64"))], ("darwin", "arm64")));
    let wasm32 = PlatformAssetTarget { wasm32: true, ..PlatformAssetTarget::default() };
    assert!(!skipped_on(vec![variant(wasm32)], ("darwin", "arm64")));
}

/// A host the installer would find no archive for skips the package, though
/// a variant names its platform: the variant's floor is above the host's.
#[test]
fn an_optional_package_whose_only_fitting_variant_needs_a_newer_host_is_skipped() {
    let floored = PlatformAssetTarget { napi: Some(u32::MAX), ..target("linux", "x64") };
    assert!(skipped_on(vec![variant(floored)], ("linux", "x64")));
}

/// A patched package's published engines are not checked under
/// `engineStrict`, but the platforms its variants run on still are.
#[test]
fn a_patched_optional_package_none_of_whose_variants_fits_is_skipped() {
    let patched = SnapshotEntry { optional: true, patched: Some(true), ..Default::default() };
    let linux = vec![variant(target("linux", "x64"))];
    let strict = InstallabilityHost { engine_strict: true, ..host("20.10.0", "darwin", "arm64") };
    assert!(skipped_as(patched, linux, &strict));
}
