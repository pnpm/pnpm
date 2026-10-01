use super::normalize_symlink_target;
use crate::{
    CafsFileInfo, PackageFilesIndex, SYMLINK_MODE, SideEffectsDiff, SideEffectsOverlay, StoreDir,
    build_file_maps_from_index,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn normalizes_recordable_targets() {
    for (link_path, target, expected) in [
        ("bin/git-add", "git", "git"),
        ("bin/git-add", "./git", "git"),
        ("bin/lib", "../lib", "../lib"),
        ("a/b/c", "../../d/e", "../../d/e"),
        ("a/b/c", "..//d/./e", "../d/e"),
    ] {
        assert_eq!(
            normalize_symlink_target(link_path, target).as_deref(),
            Some(expected),
            "{link_path} -> {target}",
        );
    }
}

#[test]
fn rejects_unrecordable_targets() {
    for (link_path, target) in [
        ("link", "../outside"),
        ("a/link", "../../outside"),
        ("link", "/etc/passwd"),
        ("link", "C:/Windows"),
        ("link", r"dir\file"),
        ("link", "a/../../outside"),
        ("a/link", ".."),
        ("link", "."),
        ("link", "node_modules/dep"),
        ("link", "sub/node_modules/dep"),
        ("node_modules/link", "target"),
        ("package.json", "manifest.json"),
        ("PACKAGE.JSON", "manifest.json"),
        ("Node_Modules/link", "target"),
        ("link", "NODE_MODULES/dep"),
        ("../../tmp/escape", "target"),
        ("a/../../escape", "target"),
        ("/tmp/escape", "target"),
        ("a//link", "target"),
        (r"a\link", "target"),
        ("link", ""),
    ] {
        assert_eq!(normalize_symlink_target(link_path, target), None, "{link_path} -> {target}");
    }
}

/// A package index whose side-effects entry `engine` adds `links` as recorded
/// symlinks and `files` as regular files over a base holding `base`.
fn index_with_links(
    store_dir: &StoreDir,
    links: &[(&str, &str)],
    (files, base, deleted): (&[&str], &[&str], &[&str]),
) -> PackageFilesIndex {
    let add = |content: &str, mode: u32| {
        let (_path, hash) =
            store_dir.write_cas_file(content.as_bytes(), false).expect("write CAFS file");
        CafsFileInfo {
            digest: format!("{hash:x}"),
            mode,
            size: content.len() as u64,
            checked_at: None,
        }
    };
    let mut added: HashMap<String, CafsFileInfo> = files
        .iter()
        .map(|path| ((*path).to_string(), add(path, 0o644)))
        .collect();
    for (path, target) in links {
        added.insert((*path).to_string(), add(target, SYMLINK_MODE));
    }
    PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files: base
            .iter()
            .map(|path| ((*path).to_string(), add(path, 0o644)))
            .collect(),
        side_effects: Some(HashMap::from([(
            "engine".to_string(),
            SideEffectsDiff {
                added: Some(added),
                deleted: (!deleted.is_empty()).then(|| {
                    deleted
                        .iter()
                        .map(|path| (*path).to_string())
                        .collect()
                }),
                remote_origin: None,
            },
        )])),
        remote_side_effects_quarantine: None,
    }
}

fn overlay_of(store_dir: &StoreDir, index: PackageFilesIndex) -> Option<SideEffectsOverlay> {
    build_file_maps_from_index(store_dir, index).side_effects_maps
        .and_then(|mut maps| maps.remove("engine"))
}

#[cfg(unix)]
#[test]
fn overlay_separates_symlinks_from_files() {
    let tmp = tempdir().expect("create store dir");
    let store_dir = StoreDir::new(tmp.path());
    let index = index_with_links(
        &store_dir,
        &[("bin/git-add", "git"), ("lib", "src"), ("replaced.js", "src/index.js")],
        (
            &["bin/git", "src/index.js"],
            &["package.json", "lib/index.js", "replaced.js"],
            &["lib/index.js"],
        ),
    );

    let overlay = overlay_of(&store_dir, index).expect("overlay restores");

    let mut files: Vec<&String> = overlay.files.keys().collect();
    files.sort();
    assert_eq!(files, ["bin/git", "package.json", "src/index.js"]);
    assert_eq!(
        overlay.symlinks,
        HashMap::from([
            ("bin/git-add".to_string(), "git".to_string()),
            ("lib".to_string(), "src".to_string()),
            ("replaced.js".to_string(), "src/index.js".to_string()),
        ]),
    );
}

#[test]
fn overlay_drops_an_entry_whose_symlink_leaves_the_package() {
    let tmp = tempdir().expect("create store dir");
    let store_dir = StoreDir::new(tmp.path());
    let index = index_with_links(&store_dir, &[("bin/git-add", "../../outside")], (&[], &[], &[]));

    assert_eq!(overlay_of(&store_dir, index), None);
}

#[test]
fn overlay_drops_an_entry_that_writes_a_file_below_a_symlink() {
    let tmp = tempdir().expect("create store dir");
    let store_dir = StoreDir::new(tmp.path());
    let index = index_with_links(&store_dir, &[("lib", "src")], (&[], &["lib/index.js"], &[]));

    assert_eq!(overlay_of(&store_dir, index), None);
}

#[test]
fn overlay_drops_an_entry_that_creates_a_symlink_below_another() {
    let tmp = tempdir().expect("create store dir");
    let store_dir = StoreDir::new(tmp.path());
    let index =
        index_with_links(&store_dir, &[("lib", "src"), ("lib/escape", "../x")], (&[], &[], &[]));

    assert_eq!(overlay_of(&store_dir, index), None);
}
