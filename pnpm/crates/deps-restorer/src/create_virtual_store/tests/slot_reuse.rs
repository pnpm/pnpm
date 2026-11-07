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
    assert!(
        !reuse
            .must_replace(&key())
            .expect("compare unchanged slot")
    );
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
    assert!(
        reuse
            .must_replace(&key())
            .expect("compare changed slot")
    );
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
        assert!(
            reuse
                .must_replace(&key())
                .expect("compare forced slot"),
            "force must replace even unchanged packages"
        );
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
    assert!(
        !reuse
            .must_replace(&key())
            .expect("compare unchanged slot")
    );
}

/// `dep-a` with an `any` archive of integrity `OLD` and a variant for the
/// host of integrity `NEW`.
fn variations_packages() -> HashMap<PackageKey, PackageMetadata> {
    let host = host_platform_selector();
    let mut metadata = metadata_with_integrity(OLD);
    let any = metadata.resolution.clone();
    let host_variant = PlatformAssetResolution {
        resolution: metadata_with_integrity(NEW).resolution,
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

/// A slot holding an archive whose `package.json` names no variant, with
/// the linked-archive record `recorded` when given.
fn write_slot(layout: &VirtualStoreLayout, recorded: Option<&str>) {
    let slot = layout.slot_dir(&key());
    let package_dir = slot.join("node_modules/dep-a");
    fs::create_dir_all(&package_dir).expect("create slot");
    fs::write(package_dir.join("package.json"), r#"{"name":"dep-a"}"#).expect("write manifest");
    let record = slot.join(".pnpm-linked-archive");
    match recorded {
        Some(integrity) => fs::write(record, integrity).expect("write record"),
        None => {
            let _ = fs::remove_file(record);
        }
    }
}

/// The slot path does not name the archive, so a slot is replaced when the
/// integrity recorded for the archive it holds differs from the one the
/// lockfile lists for the archive the install links, or when it has none.
#[test]
fn a_slot_holding_another_archive_is_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let layout = layout(dir.path());
    let packages = variations_packages();
    let reuse = SlotReuse {
        packages: &packages,
        current_packages: Some(&packages),
        force: false,
        layout: &layout,
        supported_architectures: None,
    };

    write_slot(&layout, Some(OLD));
    assert!(
        reuse
            .must_replace(&key())
            .expect("compare archive variant"),
        "the any archive's slot is replaced by the host's variant"
    );
    write_slot(&layout, None);
    assert!(
        reuse
            .must_replace(&key())
            .expect("compare missing archive"),
        "a slot without a record is replaced"
    );
    write_slot(&layout, Some(NEW));
    assert!(
        !reuse
            .must_replace(&key())
            .expect("compare linked variant"),
        "the linked variant's slot is kept"
    );
}

/// Only a slot this pass imported is recorded: one the install skips, such as
/// an installability-skipped snapshot whose old slot is still on disk, keeps
/// no record, so the install that later imports it replaces it.
#[test]
fn a_slot_the_install_did_not_import_is_not_recorded() {
    use crate::create_virtual_store::linked_archives::record_linked_archives;
    let dir = tempfile::tempdir().expect("tempdir");
    let layout = layout(dir.path());
    let packages = variations_packages();
    write_slot(&layout, None);
    let record = layout
        .slot_dir(&key())
        .join(".pnpm-linked-archive");
    let reuse = SlotReuse {
        packages: &packages,
        current_packages: Some(&packages),
        force: false,
        layout: &layout,
        supported_architectures: None,
    };

    record_linked_archives(&layout, (&[], &packages), None, &std::collections::HashSet::new())
        .expect("skip an unimported slot");
    assert!(!record.exists(), "a skipped slot is not certified");
    assert!(
        reuse
            .must_replace(&key())
            .expect("compare unrecorded slot"),
        "the slot is imported once the snapshot is installed"
    );

    record_linked_archives(&layout, (&[key()], &packages), None, &std::collections::HashSet::new())
        .expect("record imported slot");
    assert_eq!(fs::read_to_string(&record).expect("an imported slot is recorded"), NEW);
    assert!(
        !reuse
            .must_replace(&key())
            .expect("compare recorded slot")
    );
}
