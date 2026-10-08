use super::PinnedToolchain;
use pnpm_install_coordinator::PreparedInstall as _;
use pnpm_rust_toolchain::InstalledToolchain;
use std::{fs, path::Path};

fn pinned(project: &Path, toolchain: &Path) -> PinnedToolchain {
    PinnedToolchain {
        file: project.join("rust-toolchain.toml"),
        toolchain: Some(InstalledToolchain { dir: toolchain.to_path_buf() }),
        replaced: None,
    }
}

fn link_target(project: &Path) -> std::path::PathBuf {
    dunce::canonicalize(project.join(".pnpm/rust")).unwrap()
}

#[test]
fn rollback_relinks_the_toolchain_publishing_replaced() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let (before, after) = (root.path().join("1.95.0"), root.path().join("1.96.0"));
    for dir in [&project, &before, &after] {
        fs::create_dir_all(dir).unwrap();
    }
    super::link(&project, &before).unwrap();
    let mut pin = pinned(&project, &after);

    pin.publish().unwrap();
    assert_eq!(link_target(&project), dunce::canonicalize(&after).unwrap());
    pin.rollback().unwrap();

    assert_eq!(link_target(&project), dunce::canonicalize(&before).unwrap());
}

#[test]
fn rollback_removes_a_link_publishing_created() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let toolchain = root.path().join("1.96.0");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&toolchain).unwrap();
    let mut pin = pinned(&project, &toolchain);

    pin.publish().unwrap();
    pin.rollback().unwrap();

    assert!(fs::symlink_metadata(project.join(".pnpm/rust")).is_err());
}

#[cfg(unix)]
#[test]
fn rollback_restores_a_link_whose_target_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let (gone, toolchain) = (root.path().join("gone"), root.path().join("1.96.0"));
    for dir in [&project, &gone, &toolchain] {
        fs::create_dir_all(dir).unwrap();
    }
    super::link(&project, &gone).unwrap();
    fs::remove_dir(&gone).unwrap();
    let mut pin = pinned(&project, &toolchain);

    pin.publish().unwrap();
    pin.rollback().unwrap();

    let restored = fs::read_link(project.join(".pnpm/rust")).unwrap();
    assert_eq!(project.join(".pnpm").join(restored), project.join(".pnpm/../../gone"));
}
