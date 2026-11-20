use super::drive_kind_from_connection_result;

/// A status that does not identify the drive comes back as `None`, so a
/// caller cannot read a failure as an answer about the drive.
#[test]
fn connection_results_classify_drives() {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED,
        ERROR_BAD_DEVICE,
        ERROR_CONNECTION_UNAVAIL,
        ERROR_EXTENDED_ERROR,
        ERROR_MORE_DATA,
        ERROR_NO_NET_OR_BAD_PATH,
        ERROR_NO_NETWORK,
        ERROR_NOT_CONNECTED,
        NO_ERROR,
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
