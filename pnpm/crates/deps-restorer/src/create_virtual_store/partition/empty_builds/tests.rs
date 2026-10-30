use super::restorable_overlays;
use crate::{AllowBuildPolicy, create_virtual_store::partition::Rebuilds};
use pnpm_store_dir::{SideEffectsDiff, SideEffectsOverlay};
use pnpm_tarball::PrefetchResult;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

fn prefetch() -> PrefetchResult {
    let files = HashMap::from([("index.js".to_string(), PathBuf::from("/store/index"))]);
    let built = SideEffectsOverlay::from(HashMap::from([(
        "built.json".to_string(),
        PathBuf::from("/store/built"),
    )]));
    let empty = SideEffectsDiff { added: None, deleted: None, remote_origin: None };
    let changed = SideEffectsDiff { added: Some(HashMap::new()), ..empty.clone() };
    let mut prefetch = PrefetchResult::default();
    prefetch.cas_paths.insert("row".to_string(), Arc::new(files));
    prefetch.side_effects_maps.insert(
        "row".to_string(),
        Arc::new(HashMap::from([("built".to_string(), built)])),
    );
    prefetch.side_effects.insert(
        "row".to_string(),
        Arc::new(HashMap::from([("built".to_string(), changed), ("empty".to_string(), empty)])),
    );
    prefetch
}

#[test]
fn restores_an_empty_row_from_the_package_files_only_when_asked() {
    let prefetch = prefetch();
    let markers = HashSet::new();
    let allow_build_policy = AllowBuildPolicy::default();
    for restores_empty_builds in [false, true] {
        let rebuilds = Rebuilds {
            markers: &markers,
            allow_build_policy: &allow_build_policy,
            known_requires_build: None,
            known_bins: None,
            restores_empty_builds,
        };
        let overlays = restorable_overlays("row", &prefetch, &rebuilds).unwrap();
        dbg!(restores_empty_builds, &overlays);
        assert!(overlays["built"].files.contains_key("built.json"));
        assert_eq!(
            overlays.get("empty").map(|overlay| &overlay.files),
            restores_empty_builds.then(|| &*prefetch.cas_paths["row"]),
        );
    }
}
