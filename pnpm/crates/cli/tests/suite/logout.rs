use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

/// A token an earlier pnpm wrote to `auth.ini` is still revoked and removed:
/// the binary resolves `auth.ini` from
/// `XDG_CONFIG_HOME`, reads the token, sends the revoke `DELETE` to the
/// registry, and rewrites `auth.ini` without the token. Exercises the
/// CLI adapter (`LogoutArgs::run`) and the production `Host` provider
/// that the `Sys`-fake unit tests bypass.
#[test]
fn logout_revokes_token_and_removes_it_from_auth_ini() {
    const TOKEN: &str = "secret-token";
    let mut server = mockito::Server::new();
    let mock = server.mock("DELETE", "/-/user/token/secret-token").with_status(200).create();
    let registry = server.url();
    let host = registry.strip_prefix("http://").expect("mockito serves http");
    let token_key = format!("//{host}/:_authToken");

    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let config_home = root.path().join("config");
    let pnpm_dir = config_home.join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create config/pnpm");
    fs::write(pnpm_dir.join("auth.ini"), format!("{token_key}={TOKEN}\n")).expect("seed auth.ini");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("HOME", root.path())
        .with_args(["logout", "--registry", &registry])
        .output()
        .expect("run pacquet logout");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("Logged out of {registry}/")), "stdout: {stdout}");
    mock.assert();
    let remaining = fs::read_to_string(pnpm_dir.join("auth.ini")).expect("read auth.ini");
    assert!(!remaining.contains(TOKEN), "token should be removed: {remaining:?}");
}

/// `pacquet logout` with no configured token exits non-zero and reports
/// `ERR_PNPM_NOT_LOGGED_IN`.
#[test]
fn logout_errors_when_not_logged_in() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let config_home = root.path().join("config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create config/pnpm");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("HOME", root.path())
        .with_args(["logout", "--registry", "https://registry.npmjs.org/"])
        .output()
        .expect("run pacquet logout");

    assert!(!output.status.success(), "expected a non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Not logged in to https://registry.npmjs.org/"), "stderr: {stderr}");
}

/// End-to-end `pacquet logout` against the file `pnpm login` writes now: the
/// binary reads the token out of `config.yaml`'s `_auth`, revokes it, and
/// takes the registry's entry back out of the document.
#[test]
fn logout_removes_the_token_from_config_yaml() {
    const TOKEN: &str = "config-yaml-token";
    let mut server = mockito::Server::new();
    let mock = server.mock("DELETE", &*format!("/-/user/token/{TOKEN}")).with_status(200).create();
    let registry = server.url();

    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let config_home = root.path().join("config");
    let pnpm_dir = config_home.join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create config/pnpm");
    fs::write(
        pnpm_dir.join("config.yaml"),
        format!(
            "nodeLinker: hoisted\n_auth:\n  {registry}/:\n    '@': {{ authToken: {TOKEN} }}\n",
        ),
    )
    .expect("seed config.yaml");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("HOME", root.path())
        .with_args(["logout", "--registry", &registry])
        .output()
        .expect("run pacquet logout");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    mock.assert();
    let remaining = fs::read_to_string(pnpm_dir.join("config.yaml")).expect("read config.yaml");
    assert!(!remaining.contains(TOKEN), "the token should be removed: {remaining:?}");
    assert!(
        remaining.contains("nodeLinker: hoisted"),
        "the settings around the credential must survive: {remaining:?}",
    );
}

/// A registry spelled with an uppercase scheme and its default port is the
/// same registry to `pnpm login`, which canonicalizes before recording. Logout
/// has to reach the same conclusion, or the token it just revoked stays in the
/// file under a name it never looks up.
#[test]
fn logout_matches_the_registry_however_the_url_is_spelled() {
    const TOKEN: &str = "spelling-token";
    let mut server = mockito::Server::new();
    let mock = server.mock("DELETE", &*format!("/-/user/token/{TOKEN}")).with_status(200).create();
    let registry = server.url();
    let host = registry.strip_prefix("http://").expect("mockito serves http");

    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let config_home = root.path().join("config");
    let pnpm_dir = config_home.join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create config/pnpm");
    fs::write(
        pnpm_dir.join("config.yaml"),
        format!("_auth:\n  {registry}/:\n    '@': {{ authToken: {TOKEN} }}\n"),
    )
    .expect("seed config.yaml");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("HOME", root.path())
        .with_args(["logout", "--registry", &format!("HTTP://{}/", host.to_uppercase())])
        .output()
        .expect("run pacquet logout");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    mock.assert();
    let remaining = fs::read_to_string(pnpm_dir.join("config.yaml")).expect("read config.yaml");
    assert!(!remaining.contains(TOKEN), "the token should be removed: {remaining:?}");
}
