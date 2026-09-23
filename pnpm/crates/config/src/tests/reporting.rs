use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe, OsString, Path,
    PathBuf, assert_eq, capture_warnings, fs, io, load_with_project_and_user, tempdir, write_file,
};
use crate::{api::FsReadFile, auth_sources::npmrc_source, npmrc_auth::NpmrcAuth};

/// The rescope warning names the file it read and every key it pinned,
/// so a user can find and migrate the offending line.
#[test]
pub fn unscoped_creds_warn_naming_the_source_file_and_each_key() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "_auth=dXNlcjpwYXNz\nusername=alice\n_password=cGFzcw==\n");

    let warnings = capture_warnings(|| drop(load_with_project_and_user("", user_file.clone())));

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("Unscoped per-registry settings"))
        .expect("deprecation warning");
    for key in ["_auth", "username", "_password"] {
        assert!(warning.contains(key), "{warning:?} should name {key:?}");
    }
    assert!(
        warning.contains(&user_file.display().to_string()),
        "{warning:?} should name the source file",
    );
}

/// URL-scoped credentials are already pinned by construction, so they
/// pass through without a deprecation warning.
#[test]
pub fn url_scoped_creds_do_not_warn() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://example.com/\n//example.com/:_authToken=secret\n");

    let warnings = capture_warnings(|| drop(load_with_project_and_user("", user_file)));

    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("Unscoped per-registry settings")),
        "{warnings:?} should not contain a deprecation warning",
    );
}

/// `pnpm config get` / `pnpm config list` report the pinned spelling:
/// the rescope rewrites the raw INI keys alongside the structured
/// credentials, so no unscoped key survives into the reported config.
#[test]
pub fn rescoped_creds_are_reported_under_their_pinned_key() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n_authToken=user-secret\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(
        config.raw_auth_config.get("//trusted.example.com/:_authToken").map(String::as_str),
        Some("user-secret"),
    );
    assert!(!config.raw_auth_config.contains_key("_authToken"));
}

#[test]
pub fn unreadable_npmrc_becomes_a_source_carrying_its_warning() {
    struct PermissionDenied;
    impl FsReadFile for PermissionDenied {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    let project = tempdir().expect("project tempdir");
    let path = project.path().join(".npmrc");
    write_file(&path, "registry=https://example.com/\n");

    let source = npmrc_source::<PermissionDenied>(&path, |_| NpmrcAuth::default())
        .expect("an unreadable file is still a source");

    assert_eq!(source.warnings.len(), 1);
    let warning = source.warnings[0].clone();
    assert!(
        warning.starts_with(&format!(r#"Issue while reading "{}". "#, path.display())),
        "{warning:?} should name the unreadable file",
    );
    let mut config = Config::default();
    source.apply_to::<HostNoHome>(&mut config);
    assert_eq!(config.npmrc_warnings, vec![warning]);
}

#[test]
pub fn missing_npmrc_does_not_warn() {
    let auth = tempdir().expect("auth tempdir");
    let config = load_with_project_and_user("", auth.path().join("missing-npmrc"));
    assert_eq!(config.npmrc_warnings, Vec::<String>::new());
}

#[test]
pub fn npmrc_with_invalid_utf8_is_still_read() {
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), b"# \xff\nregistry=https://example.com/\n")
        .expect("write .npmrc");

    let config = Config::default().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.registry, "https://example.com/");
    assert_eq!(config.npmrc_warnings, Vec::<String>::new());
}

#[test]
pub fn unresolved_env_placeholder_keeps_the_rest_of_the_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(
        &user_file,
        "//reg.example.com/:_authToken=${PNPM_TEST_UNSET_5065}\nregistry=https://example.com/\n",
    );

    set_fake_env(&[("PNPM_CONFIG_NPMRC_AUTH_FILE", user_file.to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://example.com/");
    assert!(
        config.npmrc_warnings
            .iter()
            .any(|warning| warning.contains("${PNPM_TEST_UNSET_5065}")),
        "{:?} should report the unresolved placeholder",
        config.npmrc_warnings,
    );
}
