use super::{Config, NoEnv, NpmrcAuth, Path, TEST_CA_PEM, assert_eq};

#[test]
fn ignores_comments_and_empty_lines() {
    let ini = "
# this is a comment
; another comment

registry=https://r.example
# trailing comment
";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
}

#[test]
fn ignores_malformed_lines() {
    let ini = "not_a_key_value\nregistry=https://r.example\n=orphan_equals\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
}

#[test]
fn parses_cafile_path_from_ini() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=/etc/pacquet/ca.pem\n", Path::new(""));
    assert_eq!(auth.cafile.as_deref(), Some("/etc/pacquet/ca.pem"));
}

#[test]
fn cafile_empty_value_passes_through_unchanged() {
    // An explicit `cafile=` (empty) means "no cafile". Joining the
    // npmrc dir onto an empty path would incorrectly load the dir
    // itself, so empty must short-circuit.
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=\n", npmrc_dir.path());
    assert_eq!(auth.cafile.as_deref(), Some(""));
}

// End-to-end regression for <https://github.com/pnpm/pnpm/issues/11624>.
#[test]
fn cafile_relative_path_loads_ca_from_disk_via_apply() {
    use std::io::Write;
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let certs_dir = npmrc_dir.path().join("certs");
    std::fs::create_dir_all(&certs_dir).expect("certs dir");
    let mut ca_file = std::fs::File::create(certs_dir.join("ca.pem")).expect("create ca.pem");
    ca_file.write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=certs/ca.pem\n", npmrc_dir.path());
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 1, "tls.ca={:?}", config.tls.ca);
    assert!(config.tls.ca[0].contains("BEGIN CERTIFICATE"));
}

#[test]
fn cafile_not_found_is_silently_treated_as_unset() {
    let auth = NpmrcAuth {
        cafile: Some("/nonexistent/path/to/ca.pem".to_string()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert!(config.tls.ca.is_empty(), "missing cafile must not produce CAs");
}

#[test]
fn inline_ca_and_cafile_concatenate() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let auth = NpmrcAuth {
        ca: vec![TEST_CA_PEM.to_string()],
        cafile: Some(tmp.path().to_string_lossy().into_owned()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    // Inline first, cafile second — same ordering pnpm produces.
    assert_eq!(config.tls.ca.len(), 2, "tls.ca={:?}", config.tls.ca);
}

#[test]
fn parses_scoped_cafile_reads_from_disk() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let ini = format!("//reg.example.com/:cafile={}\n", tmp.path().display());
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    let ca = entry.ca.as_deref().expect("ca set");
    assert!(ca.contains("BEGIN CERTIFICATE"), "expected PEM contents from cafile: {ca:?}");
}

#[test]
fn parses_scoped_cafile_missing_silently_dropped() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//reg.example.com/:cafile=/nonexistent/path/ca.pem\n",
        Path::new(""),
    );
    // Either the entry doesn't exist, or it exists with `ca = None`.
    // `PerRegistryTls::from_map` filters all-`None` entries later;
    // here the parse-time behavior is "no entry written".
    assert!(
        auth.tls_by_uri.get("//reg.example.com/").is_none_or(|entry| entry.ca.is_none()),
        "missing cafile must not produce a non-None ca slot: {:?}",
        auth.tls_by_uri,
    );
}

#[test]
fn scoped_inline_and_file_share_same_slot_last_wins() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(b"FROM-FILE").expect("write");
    let ini = format!(
        "//reg.example.com/:cert=inline\n//reg.example.com/:certfile={}\n",
        tmp.path().display(),
    );
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.cert.as_deref(), Some("FROM-FILE"));
}
