use super::{
    Algorithm, ArchiveStoreProjection, CafsFileInfo, Integrity, SharedReadonlyStoreIndex, StoreDir,
};
use sha2::{Digest, Sha512};
use std::{io::Read, path::Path};

pub(in crate::cargo_deps) fn slot_is_complete_with_index(
    slot: &Path,
    name: &str,
    version: &str,
    checksum: &str,
    store_dir: &StoreDir,
    index: Option<&SharedReadonlyStoreIndex>,
) -> bool {
    let Ok(slot_metadata) = std::fs::symlink_metadata(slot) else {
        return false;
    };
    if !slot_metadata.is_dir() {
        return false;
    }
    let Some(manifest) = read_slot_manifest(slot, checksum) else {
        return false;
    };
    let Some(index) = index else {
        return false;
    };
    let Ok(integrity) = Integrity::from_hex(checksum, Algorithm::Sha256) else {
        return false;
    };
    let package_id = format!("crate:{name}@{version}");
    let key =
        ArchiveStoreProjection::RawArchive.store_index_key(&integrity.to_string(), &package_id);
    let Ok(index) = index.lock() else {
        return false;
    };
    let Ok(Some(indexed)) = index.get(&key) else {
        return false;
    };
    !indexed.files.is_empty()
        && manifest.len() == indexed.files.len()
        && indexed.files
            .iter()
            .all(|(file, info)| {
                manifest.contains_key(file) && slot_file_matches_cas(slot, file, info, store_dir)
            })
}

fn read_slot_manifest(
    slot: &Path,
    checksum: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let marker = slot.join(".cargo-checksum.json");
    let Ok(marker_metadata) = std::fs::symlink_metadata(&marker) else {
        return None;
    };
    if !marker_metadata.is_file() {
        return None;
    }
    let Ok(manifest) = std::fs::read(&marker) else {
        return None;
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest) else {
        return None;
    };
    if manifest.get("package").and_then(serde_json::Value::as_str) != Some(checksum) {
        return None;
    }
    manifest
        .get("files")
        .and_then(serde_json::Value::as_object)
        .cloned()
}

fn slot_file_matches_cas(
    slot: &Path,
    file: &str,
    info: &CafsFileInfo,
    store_dir: &StoreDir,
) -> bool {
    safe_slot_file_path(file)
        && slot_parent_directories_are_real(slot, file)
        && slot_file_contents_match_cas(slot, file, info, store_dir)
}

fn safe_slot_file_path(file: &str) -> bool {
    Path::new(file)
        .components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn slot_parent_directories_are_real(slot: &Path, file: &str) -> bool {
    let mut parent = slot.to_path_buf();
    let Some(relative_parent) = Path::new(file).parent() else {
        return true;
    };
    for component in relative_parent.components() {
        parent.push(component.as_os_str());
        let Ok(metadata) = std::fs::symlink_metadata(&parent) else {
            return false;
        };
        if !metadata.is_dir() {
            return false;
        }
    }
    true
}

fn slot_file_contents_match_cas(
    slot: &Path,
    file: &str,
    info: &CafsFileInfo,
    store_dir: &StoreDir,
) -> bool {
    let path = slot.join(file);
    let Some(cas_path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
        return false;
    };
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() != info.size {
        return false;
    }
    let Ok(cas_metadata) = std::fs::symlink_metadata(&cas_path) else {
        return false;
    };
    if !cas_metadata.is_file() || cas_metadata.len() != info.size {
        return false;
    }
    cas_file_matches_digest(&cas_path, &info.digest)
        && pnpm_crypto_hash::create_hex_hash_from_file(&path)
            .is_ok_and(|slot_checksum| {
                pnpm_crypto_hash::create_hex_hash_from_file(&cas_path)
                    .is_ok_and(|cas_checksum| slot_checksum == cas_checksum)
            })
}

fn cas_file_matches_digest(path: &Path, digest: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hasher = Sha512::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let Ok(read) = file.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            return format!("{:x}", hasher.finalize()) == digest;
        }
        hasher.update(&buffer[..read]);
    }
}
