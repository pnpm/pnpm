use super::{assert_eq, capture_warnings, load_with_project_and_user, tempdir, write_file};

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
        !warnings.iter().any(|warning| warning.contains("Unscoped per-registry settings")),
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
