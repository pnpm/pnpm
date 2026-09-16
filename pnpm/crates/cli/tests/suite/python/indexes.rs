mod rules;

use super::{
    add_python_settings, assert_failure_contains, pacquet_in, project, python, serve,
    serve_with_index_auth, wheel,
};
use assert_cmd::assert::OutputAssertExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
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
    let mut extra_url: url::Url = extra.url().parse().unwrap();
    extra_url.set_username("extra-user").unwrap();
    extra_url
        .set_password(Some("extra-secret"))
        .unwrap();
    add_python_settings(
        root.path(),
        &format!("  extraIndexUrls: ['{}/simple/']\n", extra_url.as_str().trim_end_matches('/')),
    );
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
    add_python_settings(root.path(), &format!("  extraIndexUrls: ['{}/simple/']\n", extra.url()));
    assert_failure_contains(pacquet_in(root.path()).arg("install"), "403 Forbidden");
    unused.assert_async().await;
    denied.assert_async().await;
}

#[tokio::test]
async fn credentialless_descendant_indexes_do_not_receive_parent_index_credentials() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let authorization = format!("Basic {}", STANDARD.encode("parent:secret"));
    let _alpha = serve_with_index_auth(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "", &[]))],
        Some(&authorization),
    )
    .await;
    let missing = server
        .mock("GET", "/simple/public/alpha/")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(404)
        .create_async()
        .await;
    let mut index: url::Url = server.url().parse().unwrap();
    index.set_username("parent").unwrap();
    index
        .set_password(Some("secret"))
        .unwrap();
    project(root.path(), index.as_str().trim_end_matches('/'), &["alpha"]);
    add_python_settings(
        root.path(),
        &format!("  extraIndexUrls: ['{}/simple/public/']\n", server.url()),
    );
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    missing.assert_async().await;
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
    let mut alice_url: url::Url = format!("{}/simple/", extra.url()).parse().unwrap();
    alice_url.set_username("user").unwrap();
    alice_url
        .set_password(Some("alice-secret"))
        .unwrap();
    let mut bob_url = alice_url.clone();
    bob_url
        .set_password(Some("bob-secret"))
        .unwrap();
    add_python_settings(root.path(), &format!("  extraIndexUrls: ['{alice_url}']\n"));
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let settings = root.path().join("pnpm-workspace.yaml");
    let alice_settings = fs::read_to_string(&settings).unwrap();
    fs::write(&settings, alice_settings.replace(alice_url.as_str(), bob_url.as_str())).unwrap();
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
    fs::write(settings, alice_settings).unwrap();
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha; assert alpha.VERSION == '1.0'"])
        .assert()
        .success();
    let mut anonymous = alice_url.clone();
    anonymous.set_username("").unwrap();
    anonymous.set_password(None).unwrap();
    let legacy = root.path().join("cache/python-index-v2");
    fs::create_dir_all(&legacy).unwrap();
    let page_url = anonymous.join("alpha/").unwrap();
    fs::write(
        legacy.join(format!("{:x}.json", Sha256::digest(page_url.as_str().as_bytes()))),
        serde_json::json!({"url": page_url, "body": {"files": []}, "missing": true}).to_string(),
    )
    .unwrap();
    let settings = root.path().join("pnpm-workspace.yaml");
    let alice_settings = fs::read_to_string(&settings).unwrap();
    fs::write(settings, alice_settings.replace(alice_url.as_str(), anonymous.as_str())).unwrap();
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--offline"]),
        "not cached for offline resolution",
    );
    missing.assert_async().await;
}
