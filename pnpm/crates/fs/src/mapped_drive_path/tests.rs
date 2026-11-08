use super::mapped_drive_path;
use std::path::{Path, PathBuf};

#[test]
fn mapped_unc_path_uses_its_original_drive() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share\project\node_modules"),
            'H',
            Path::new(r"\\server\share"),
        ),
        Some(PathBuf::from(r"H:\project\node_modules")),
    );
}

#[test]
fn mapped_verbatim_unc_path_uses_its_original_drive() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\?\UNC\server\share\project"),
            'H',
            Path::new(r"\\server\share")
        ),
        Some(PathBuf::from(r"H:\project")),
    );
}

#[test]
fn mapped_unc_share_root_maps_to_the_drive_root() {
    assert_eq!(
        mapped_drive_path(Path::new(r"\\server\share"), 'H', Path::new(r"\\server\share\")),
        Some(PathBuf::from(r"H:\")),
    );
}

#[test]
fn mapped_unc_path_under_a_nested_share_maps_below_the_drive_root() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share\nested\project"),
            'J',
            Path::new(r"\\server\share\nested"),
        ),
        Some(PathBuf::from(r"J:\project")),
    );
}

#[test]
fn mapped_unc_path_matches_share_names_without_case_sensitivity() {
    assert_eq!(
        mapped_drive_path(Path::new(r"\\SERVER\SHARE\project"), 'H', Path::new(r"\\server\share")),
        Some(PathBuf::from(r"H:\project")),
    );
}

#[test]
fn mapped_unc_path_does_not_match_a_partial_share_name() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share-other\project"),
            'H',
            Path::new(r"\\server\share")
        ),
        None,
    );
}

#[test]
fn mapped_unc_path_does_not_match_a_path_shorter_than_the_share() {
    assert_eq!(
        mapped_drive_path(Path::new(r"\\server\share"), 'H', Path::new(r"\\server\share\nested")),
        None,
    );
}
