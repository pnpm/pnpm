use super::{Config, GetHomeDir, LinkProbe, Path, PathBuf, StoreDir, assert_eq, fs, tempdir};

/// A home store on another volume: only a directory named `mount` accepts
/// a hard link from the project.
struct MountVolume;
impl GetHomeDir for MountVolume {
    fn home_dir() -> Option<PathBuf> {
        Some(PathBuf::from("/home-volume"))
    }
}
impl LinkProbe for MountVolume {
    fn can_link_between_dirs(_: &Path, to_dir: &Path) -> bool {
        to_dir.ends_with("mount")
    }
}

fn unplaced_config() -> Config {
    let mut config = Config::new();
    config.store_dir = StoreDir::new("/home-volume/pnpm/store");
    config.skip_store_dir_resolution = true;
    config
}

#[test]
fn an_unplaced_store_moves_to_the_project_volume() {
    let tmp = tempdir().expect("create tempdir");
    let mount = fs::canonicalize(tmp.path()).expect("canonicalize tempdir").join("mount");
    let project = mount.join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let mut config = unplaced_config();
    config.place_skipped_store_dir::<MountVolume>(&project);

    assert_eq!(config.store_dir, StoreDir::new(mount.join(".pnpm-store")));
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
    assert!(!config.skip_store_dir_resolution);
}

#[test]
fn a_pinned_store_stays() {
    let tmp = tempdir().expect("create tempdir");
    let project = tmp.path().join("mount/project");
    fs::create_dir_all(&project).expect("create project dir");

    let mut config = unplaced_config();
    config.explicit_settings.insert("storeDir".to_string(), "/home-volume/pnpm/store".into());
    config.place_skipped_store_dir::<MountVolume>(&project);

    assert_eq!(config.store_dir, StoreDir::new("/home-volume/pnpm/store"));
}
