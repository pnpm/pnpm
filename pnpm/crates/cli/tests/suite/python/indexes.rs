mod rules;

use super::{
    add_python_index_searched_first, add_python_settings, assert_failure_contains, pacquet_in,
    project, python, serve, serve_with_index_auth, wheel,
};
use assert_cmd::assert::OutputAssertExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fs;

#[tokio::test]
async fn extra_indexes_have_priority_and_cache_missing_packages_for_offline_resolution() {
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
    let missing = extra
        .mock("GET", "/simple/beta/")
        .match_header("authorization", authorization.as_str())
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_index_searched_first(root.path(), &format!("{}/simple/", extra.url()));
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
    assert!(lock.contains("extra-indexes"));
    assert!(!lock.contains("extra-secret"));
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    missing.assert_async().await;
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
async fn extra_index_errors_do_not_fall_back_to_another_index() {
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
    add_python_index_searched_first(root.path(), &format!("{}/simple/", extra.url()));
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
    // Every lookup reaches the extra index first, and none of them may carry
    // the credential configured for the other origin.
    let mut anonymous = Vec::new();
    for distribution in ["alpha", "beta"] {
        anonymous.push(
            extra
                .mock("GET", format!("/simple/{distribution}/").as_str())
                .match_header("authorization", mockito::Matcher::Missing)
                .with_status(404)
                .expect(1)
                .create_async()
                .await,
        );
    }
    let _beta = serve_with_index_auth(
        &mut primary,
        "beta",
        &[("1.0", wheel("beta", "1.0", "", &[]))],
        Some(&authorization),
    )
    .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_index_searched_first(root.path(), &format!("{}/simple/", extra.url()));
    write_index_credentials(root.path(), &primary.url(), "parent:secret");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    for mock in anonymous {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn authenticated_index_caches_do_not_cross_credential_identities() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let _alpha = serve(&mut primary, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let alice = format!("Basic {}", STANDARD.encode("user:alice-secret"));
    let bob = format!("Basic {}", STANDARD.encode("user:bob-secret"));
    let missing = extra
        .mock("GET", "/simple/alpha/")
        .match_header("authorization", alice.as_str())
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _bob_alpha = serve_with_index_auth(
        &mut extra,
        "alpha",
        &[("2.0", wheel("alpha", "2.0", "", &[]))],
        Some(&bob),
    )
    .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_index_searched_first(root.path(), &format!("{}/simple/", extra.url()));
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
    missing.assert_async().await;
}

#[test]
fn an_index_may_not_carry_its_own_credentials() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "http://localhost:1", &["alpha"]);
    add_python_index_searched_first(root.path(), "http://alice:private-secret@localhost:1/simple/");
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
