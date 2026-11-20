use super::{drive_kind_from_connection_result, mapped_drive_path};
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

/// Only a result that identifies the drive is cached, so errors that say
/// nothing about the drive must come back as `None`.
#[test]
fn connection_results_classify_drives() {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_BAD_DEVICE, ERROR_CONNECTION_UNAVAIL, ERROR_EXTENDED_ERROR,
        ERROR_MORE_DATA, ERROR_NO_NET_OR_BAD_PATH, ERROR_NO_NETWORK, ERROR_NOT_CONNECTED, NO_ERROR,
    };

    for mapped in [NO_ERROR, ERROR_MORE_DATA, ERROR_CONNECTION_UNAVAIL] {
        assert_eq!(drive_kind_from_connection_result(mapped), Some(true), "{mapped}");
    }
    assert_eq!(drive_kind_from_connection_result(ERROR_NOT_CONNECTED), Some(false));
    for unknown in [
        ERROR_NO_NETWORK,
        ERROR_BAD_DEVICE,
        ERROR_NO_NET_OR_BAD_PATH,
        ERROR_EXTENDED_ERROR,
        ERROR_ACCESS_DENIED,
    ] {
        assert_eq!(drive_kind_from_connection_result(unknown), None, "{unknown}");
    }
}
