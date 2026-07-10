use super::FetchMetadataError;
use pacquet_network::redact_url_credentials;

/// The retry path logs the failing error through
/// `redact_url_credentials(&format!("{error:?}"))`. `FetchMetadataError`'s
/// derived `Debug` embeds its raw `url` field, so this guards that an
/// inline-credential registry URL (`https://user:pass@host/`) can't survive
/// into a retry log line through the error value.
#[test]
fn debug_render_redacts_embedded_url_credentials() {
    let json_error = serde_json::from_str::<u8>(r#""not a number""#).unwrap_err();
    let error = FetchMetadataError::Decode {
        url: "https://user:secret@registry.example/pkg".to_string(),
        error: json_error,
    };
    let logged = redact_url_credentials(&format!("{error:?}"));
    assert!(!logged.contains("secret"), "password must not survive: {logged}");
    assert!(!logged.contains("user:"), "userinfo must not survive: {logged}");
    assert!(logged.contains("registry.example/pkg"), "host/path retained: {logged}");
}
