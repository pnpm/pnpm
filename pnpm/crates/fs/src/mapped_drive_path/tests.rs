use super::mapped_drive_path;
use std::path::{Path, PathBuf};

#[test]
fn mapped_unc_path_uses_its_original_drive_when_available() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share\project\node_modules"),
            &[('H', PathBuf::from(r"\\server\share"))],
            Some('H'),
        ),
        Some(PathBuf::from(r"H:\project\node_modules")),
    );
}

#[test]
fn mapped_unc_path_prefers_its_original_drive_between_aliases() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share\project"),
            &[('H', PathBuf::from(r"\\server\share")), ('J', PathBuf::from(r"\\server\share")),],
            Some('H'),
        ),
        Some(PathBuf::from(r"H:\project")),
    );
}

#[test]
fn mapped_verbatim_unc_path_uses_its_original_drive() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\?\UNC\server\share\project"),
            &[('H', PathBuf::from(r"\\server\share"))],
            Some('H'),
        ),
        Some(PathBuf::from(r"H:\project")),
    );
}

#[test]
fn mapped_unc_share_root_maps_to_the_drive_root() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share"),
            &[('H', PathBuf::from(r"\\server\share\"))],
            None,
        ),
        Some(PathBuf::from(r"H:\")),
    );
}

#[test]
fn mapped_unc_path_uses_the_longest_matching_share() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share\nested\project"),
            &[
                ('H', PathBuf::from(r"\\server\share")),
                ('J', PathBuf::from(r"\\server\share\nested")),
            ],
            None,
        ),
        Some(PathBuf::from(r"J:\project")),
    );
}

#[test]
fn mapped_unc_path_matches_share_names_without_case_sensitivity() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\SERVER\SHARE\project"),
            &[('H', PathBuf::from(r"\\server\share"))],
            None,
        ),
        Some(PathBuf::from(r"H:\project")),
    );
}

#[test]
fn mapped_unc_path_does_not_match_a_partial_share_name() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share-other\project"),
            &[('H', PathBuf::from(r"\\server\share"))],
            None,
        ),
        None,
    );
}

#[test]
fn mapped_unc_path_does_not_match_a_path_shorter_than_the_share() {
    assert_eq!(
        mapped_drive_path(
            Path::new(r"\\server\share"),
            &[('H', PathBuf::from(r"\\server\share\nested"))],
            None,
        ),
        None,
    );
}

#[test]
fn mapped_unc_path_stays_unc_when_it_has_no_matching_drive() {
    assert_eq!(mapped_drive_path(Path::new(r"\\server\share\project"), &[], None), None);
}
