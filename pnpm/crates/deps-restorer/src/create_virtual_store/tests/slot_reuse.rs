use super::{super::cache_keys::SlotReuse, metadata_with_integrity, name};
use crate::{VirtualStoreLayout, platform_selection::host_platform_selector};
use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgVerPeer, PlatformAssetResolution,
    PlatformAssetTarget, VariationsResolution,
};
use std::{collections::HashMap, fs, path::Path};

const OLD: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const NEW: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

fn key() -> PackageKey {
    PackageKey {
        name: name("dep-a"),
        suffix: "1.0.0".parse::<PkgVerPeer>().expect("parse version"),
    }
}

/// A non-global virtual store at `install_state_dir`.
fn layout(install_state_dir: &Path) -> VirtualStoreLayout {
    let mut config = Config::new();
    config.enable_global_virtual_store = false;
    config.install_state_dir = install_state_dir.to_path_buf();
    VirtualStoreLayout::new(&config, None, None, None, None, None)
}

fn packages(integrity: &str) -> HashMap<PackageKey, PackageMetadata> {
    HashMap::from([(key(), metadata_with_integrity(integrity))])
}

#[test]
fn an_unchanged_integrity_keeps_the_slot() {
    let wanted = packages(OLD);
    let current = packages(OLD);
    let reuse = SlotReuse {
        packages: &wanted,
        current_packages: Some(&current),
        force: false,
        layout: &layout(Path::new("/nonexistent")),
        supported_architectures: None,
    };
    assert!(!reuse.must_replace(&key()));
}

#[test]
fn a_changed_integrity_replaces_the_slot() {
    let wanted = packages(NEW);
    let current = packages(OLD);
    let reuse = SlotReuse {
        packages: &wanted,
        current_packages: Some(&current),
        force: false,
        layout: &layout(Path::new("/nonexistent")),
        supported_architectures: None,
    };
    assert!(reuse.must_replace(&key()));
}

#[test]
fn force_replaces_the_slot_with_or_without_previous_records() {
    let wanted = packages(OLD);
    let current = packages(OLD);
    for current_packages in [None, Some(&current)] {
        let reuse = SlotReuse {
            packages: &wanted,
            current_packages,
            force: true,
            layout: &layout(Path::new("/nonexistent")),
            supported_architectures: None,
        };
        assert!(reuse.must_replace(&key()), "force must replace even unchanged packages");
    }
}

/// A first install has no records either, but its slots do not exist yet,
/// so replacing them is the import's business rather than this decision's.
#[test]
fn a_first_install_without_force_leaves_the_verdict_to_the_import() {
    let wanted = packages(NEW);
    let reuse = SlotReuse {
        packages: &wanted,
        current_packages: None,
        force: false,
        layout: &layout(Path::new("/nonexistent")),
        supported_architectures: None,
    };
    assert!(!reuse.must_replace(&key()));
}

/// `dep-a` with an `any` archive and a variant for the host.
fn variations_packages() -> HashMap<PackageKey, PackageMetadata> {
    let host = host_platform_selector();
    let mut metadata = metadata_with_integrity(OLD);
    let any = metadata.resolution.clone();
    let host_variant = PlatformAssetResolution {
        resolution: any.clone(),
        targets: vec![PlatformAssetTarget {
            os: host.os,
            cpu: host.cpu,
            libc: host.libc.filter(|libc| libc == "musl"),
            ..PlatformAssetTarget::default()
        }],
    };
    metadata.resolution = LockfileResolution::Variations(VariationsResolution::new(vec![
        PlatformAssetResolution::any(any),
        host_variant,
    ]));
    HashMap::from([(key(), metadata)])
}

/// A slot whose `package.json` declares the variant `platforms`.
fn write_slot(layout: &VirtualStoreLayout, platforms: &[&str]) {
    let package_dir = layout
        .slot_dir(&key())
        .join("node_modules/dep-a");
    fs::create_dir_all(&package_dir).expect("create slot");
    let manifest = serde_json::json!({ "name": "dep-a", "variant": { "platforms": platforms } });
    fs::write(package_dir.join("package.json"), manifest.to_string()).expect("write manifest");
}

/// The slot path does not name the archive, so a slot holding another
/// variant than the one the install links is replaced, though the lockfile
/// records the package unchanged.
#[test]
fn a_slot_holding_another_variant_is_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let layout = layout(dir.path());
    let packages = variations_packages();
    let LockfileResolution::Variations(variations) = &packages[&key()].resolution else {
        unreachable!("the package has variants");
    };
    let host_tag = variations
        .select(&host_platform_selector())
        .ok()
        .flatten()
        .and_then(PlatformAssetResolution::variant_tag)
        .expect("the host has a variant");
    let reuse = SlotReuse {
        packages: &packages,
        current_packages: Some(&packages),
        force: false,
        layout: &layout,
        supported_architectures: None,
    };

    write_slot(&layout, &["freebsd-x64"]);
    assert!(reuse.must_replace(&key()), "another variant's slot is replaced");
    write_slot(&layout, &[host_tag.as_str()]);
    assert!(!reuse.must_replace(&key()), "the linked variant's slot is kept");
}
