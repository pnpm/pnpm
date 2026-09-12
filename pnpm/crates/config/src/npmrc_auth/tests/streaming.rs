use super::{Config, NoEnv, NpmrcAuth, TEST_CA_PEM, assert_eq};

#[test]
fn cafile_trailing_garbage_is_preserved_for_downstream_parser() {
    // The split matches pnpm's `readCAFileSync`, which keeps the
    // trailing chunk of a truncated bundle. The network layer is what
    // decides an entry carries no certificate.
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let bundle = format!("{TEST_CA_PEM}\ngarbage-not-a-cert");
    tmp.as_file().write_all(bundle.as_bytes()).expect("write bundle");
    let auth = NpmrcAuth {
        cafile: Some(tmp.path().to_string_lossy().into_owned()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 2, "tls.ca={:?}", config.tls.ca);
    assert!(
        config.tls.ca[1].starts_with("garbage-not-a-cert"),
        "trailing garbage entry was not preserved: {:?}",
        config.tls.ca[1],
    );
    assert!(
        config.tls.ca[1].ends_with("-----END CERTIFICATE-----"),
        "delimiter was not re-appended to garbage entry: {:?}",
        config.tls.ca[1],
    );
}
