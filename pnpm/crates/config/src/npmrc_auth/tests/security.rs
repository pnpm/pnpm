use super::{Config, EnvVar, NoEnv, NpmrcAuth, Path, assert_eq};

#[test]
fn cascade_legacy_proxy_null_falls_through_to_env() {
    static_env!(HttpsEnv, &[("HTTPS_PROXY", "http://https-env.example:8080")]);
    let auth = NpmrcAuth::from_ini::<NoEnv>("proxy=null\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<HttpsEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https-env.example:8080"));
}

#[test]
fn cafile_absolute_path_passes_through_unchanged() {
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let abs_cafile = tempfile::NamedTempFile::new().expect("tempfile");
    let abs_path = abs_cafile.path().to_string_lossy().into_owned();
    let auth = NpmrcAuth::from_ini::<NoEnv>(&format!("cafile={abs_path}\n"), npmrc_dir.path());
    assert_eq!(auth.cafile.as_deref(), Some(abs_path.as_str()));
}

#[test]
fn scoped_n_escape_expansion_only_on_inline() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("//reg.example.com/:ca=line1\\nline2\n", Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.ca.as_deref(), Some("line1\nline2"));
}
