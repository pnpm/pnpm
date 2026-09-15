use super::{WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs};

/// Credentials belong in `.npmrc`, which is not committed. Refused after
/// parsing, not by `deny_unknown_fields`: a parse error renders the offending
/// source line verbatim, which would print the very token being refused.
#[test]
fn rejects_credentials_in_a_registry_declaration() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: {_authToken: hunter2}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("credentials in a declaration must not load")
        .to_string();
    assert!(error.contains("_authToken"), "the field is named: {error}");
    assert!(!error.contains("hunter2"), "the token must not be echoed: {error}");
}

/// A credential in the key is the same secret in the same committed file as a
/// credential in a field, so both are refused. The check runs after parsing so
/// the error carries a redacted URL instead of serde's verbatim source line.
#[test]
fn rejects_a_registry_key_that_embeds_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://ci-user-6e42:hunter2@npm.example.com/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a key with credentials must not load")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
    assert!(error.contains("npm.example.com"), "the host is still named: {error}");
}

/// `.npmrc` scopes settings with a scheme-less `//host/`, and this setting's
/// own error points users at that syntax, so it is the form they are most
/// likely to write — and it must not slip past the check.
#[test]
fn rejects_a_scheme_less_registry_key_that_embeds_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  //ci-user-6e42:hunter2@npm.example.com/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scheme-less key with credentials must not load")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
}
