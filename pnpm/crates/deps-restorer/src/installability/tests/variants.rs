//! A package published with platform variants and no archive that targets
//! `any` installs only where a variant does.

use super::{host, no_importers, snapshot_key, synthetic_metadata};
use crate::installability::{SkippedSnapshots, compute_skipped_snapshots};
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
    let key = snapshot_key("example-cli@1.0.0");
    let snapshots =
        HashMap::from([(key.clone(), SnapshotEntry { optional: true, ..Default::default() })]);
    let mut metadata = synthetic_metadata(None, None, None, None);
    metadata.resolution = LockfileResolution::Variations(VariationsResolution::new(variants));
    let packages = HashMap::from([(key.clone(), metadata)]);
    compute_skipped_snapshots::<SilentReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", os, cpu),
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
