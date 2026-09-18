mod rules;

use super::{
    TINY_BACKEND, add_python_registry, add_python_settings, assert_failure_contains, pacquet_in,
    project, python, python_project, serve, serve_with_index_auth, wheel,
};
use assert_cmd::assert::OutputAssertExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fs;

#[tokio::test]
async fn package_routes_cover_transitive_dependencies_and_offline_resolution() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let _primary_alpha =
        serve(&mut primary, "alpha", &[("9.0", wheel("alpha", "9.0", "", &[]))]).await;
    let authorization = format!("Basic {}", STANDARD.encode("extra-user:extra-secret"));
    let _extra_alpha = serve_with_index_auth(
        &mut extra,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta\n", &[]))],
        Some(&authorization),
    )
    .await;
    let _primary_beta =
        serve(&mut primary, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    let unused = extra
        .mock("GET", "/simple/beta/")
        .expect(0)
        .create_async()
        .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_registry(root.path(), &format!("{}/simple/", extra.url()), &["alpha"]);
    write_index_credentials(root.path(), &extra.url(), "extra-user:extra-secret");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta; assert alpha.VERSION == '1.0'"])
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("{lock}");
    assert!(lock.contains("registry-packages"));
    assert!(!lock.contains("extra-secret"));
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    unused.assert_async().await;
    add_python_settings(root.path(), "  constraints: ['alpha>=2']\n");
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "overrides or constraints changed",
    );
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python dependency resolution failed",
    );
}

#[tokio::test]
async fn selected_registry_errors_do_not_fall_back_to_another_index() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let unused = primary
        .mock("GET", "/simple/alpha/")
        .expect(0)
        .create_async()
        .await;
    let denied = extra
        .mock("GET", "/simple/alpha/")
        .with_status(403)
        .create_async()
        .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_registry(root.path(), &format!("{}/simple/", extra.url()), &["alpha"]);
    assert_failure_contains(pacquet_in(root.path()).arg("install"), "403 Forbidden");
    unused.assert_async().await;
    denied.assert_async().await;
}

#[tokio::test]
async fn an_index_credential_does_not_travel_to_an_index_on_another_origin() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let authorization = format!("Basic {}", STANDARD.encode("parent:secret"));
    let _alpha = serve_with_index_auth(
        &mut primary,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta\n", &[]))],
        Some(&authorization),
    )
    .await;
    let beta = serve(&mut extra, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_registry(root.path(), &format!("{}/simple/", extra.url()), &["beta"]);
    write_index_credentials(root.path(), &primary.url(), "parent:secret");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    beta.last()
        .unwrap()
        .assert_async()
        .await;
}

#[tokio::test]
async fn authenticated_index_caches_do_not_cross_credential_identities() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let _alpha = serve(&mut primary, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let alice = format!("Basic {}", STANDARD.encode("user:alice-secret"));
    let bob = format!("Basic {}", STANDARD.encode("user:bob-secret"));
    let _alice_alpha = serve_with_index_auth(
        &mut extra,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "", &[]))],
        Some(&alice),
    )
    .await;
    let _bob_alpha = serve_with_index_auth(
        &mut extra,
        "alpha",
        &[("2.0", wheel("alpha", "2.0", "", &[]))],
        Some(&bob),
    )
    .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_registry(root.path(), &format!("{}/simple/", extra.url()), &["alpha"]);
    write_index_credentials(root.path(), &extra.url(), "user:alice-secret");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let alice_npmrc = fs::read_to_string(root.path().join(".npmrc")).unwrap();
    write_index_credentials(root.path(), &extra.url(), "user:bob-secret");
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--offline"]),
        "not cached for offline resolution",
    );
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha; assert alpha.VERSION == '2.0'"])
        .assert()
        .success();
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    fs::write(root.path().join(".npmrc"), &alice_npmrc).unwrap();
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha; assert alpha.VERSION == '1.0'"])
        .assert()
        .success();
}

#[test]
fn an_index_may_not_carry_its_own_credentials() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "http://localhost:1", &["alpha"]);
    add_python_registry(root.path(), "http://alice:private-secret@localhost:1/simple/", &["alpha"]);
    let output = pacquet_in(root.path())
        .arg("install")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let message = String::from_utf8(output).unwrap();
    assert!(message.contains("ERR_PNPM_INVALID_SETTING"), "{message}");
    assert!(message.contains("credentials"), "{message}");
    assert!(!message.contains("private-secret"), "{message}");
}

/// `registries` refuses a credential in the setting itself, so an `.npmrc`
/// beside the project is where a Python index's credential lives.
fn write_index_credentials(root: &std::path::Path, index: &str, user_and_password: &str) {
    let authority = index
        .strip_prefix("http://")
        .or_else(|| index.strip_prefix("https://"))
        .expect("a mockito index URL")
        .trim_end_matches('/');
    fs::write(
        root.join(".npmrc"),
        format!("//{authority}/simple/:_auth={}\n", STANDARD.encode(user_and_password)),
    )
    .unwrap();
}

#[tokio::test]
async fn missing_private_packages_never_fall_back_to_the_default_index() {
    for transitive in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut public = mockito::Server::new_async().await;
        let mut private = mockito::Server::new_async().await;
        let _alpha = serve(
            &mut public,
            "alpha",
            &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta\n", &[]))],
        )
        .await;
        let unused = public
            .mock("GET", "/simple/beta/")
            .expect(0)
            .create_async()
            .await;
        let missing = private
            .mock("GET", "/simple/beta/")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        project(root.path(), &public.url(), &[if transitive { "alpha" } else { "beta" }]);
        add_python_registry(root.path(), &format!("{}/simple/", private.url()), &["beta"]);
        assert_failure_contains(
            pacquet_in(root.path()).arg("install"),
            "Python dependency resolution failed",
        );
        assert_failure_contains(
            pacquet_in(root.path()).args(["install", "--offline"]),
            "Python dependency resolution failed",
        );
        missing.assert_async().await;
        unused.assert_async().await;
    }
}

#[tokio::test]
async fn incompatible_private_versions_never_fall_back_to_the_default_index() {
    let root = tempfile::tempdir().unwrap();
    let mut public = mockito::Server::new_async().await;
    let mut private = mockito::Server::new_async().await;
    let unused = public
        .mock("GET", "/simple/alpha/")
        .expect(0)
        .create_async()
        .await;
    let _private = serve(&mut private, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &public.url(), &["alpha>=2"]);
    add_python_registry(root.path(), &format!("{}/simple/", private.url()), &["alpha"]);
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python dependency resolution failed",
    );
    unused.assert_async().await;
}

#[tokio::test]
async fn package_route_changes_invalidate_frozen_lockfiles() {
    let root = tempfile::tempdir().unwrap();
    let public = mockito::Server::new_async().await;
    let mut private = mockito::Server::new_async().await;
    let _private = serve(&mut private, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &public.url(), &["alpha"]);
    add_python_registry(root.path(), &format!("{}/simple/", private.url()), &["alpha"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let path = root.path().join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&path).unwrap();
    fs::write(path, yaml.replace(r#"packages: ["alpha"]"#, r#"packages: ["beta"]"#)).unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "Python index changed",
    );
}

#[tokio::test]
async fn isolated_build_dependencies_use_registry_claims() {
    let root = tempfile::tempdir().unwrap();
    let mut public = mockito::Server::new_async().await;
    let mut private = mockito::Server::new_async().await;
    let unused = public
        .mock("GET", "/simple/tinybackend/")
        .expect(0)
        .create_async()
        .await;
    let _backend = serve(
        &mut private,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", TINY_BACKEND)]))],
    )
    .await;
    project(root.path(), &public.url(), &[]);
    python_project(root.path(), "app", "dependencies = []");
    add_python_registry(root.path(), &format!("{}/simple/", private.url()), &["tinybackend"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import app"])
        .assert()
        .success();
    unused.assert_async().await;
}
