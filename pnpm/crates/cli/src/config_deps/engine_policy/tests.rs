use super::mature_pnpm_version_for_range;
use pnpm_config::{Config, PNPM_VERSION, TrustPolicy};

const OLDER: &str = "2023-01-10T08:30:00.000Z";
const OLD: &str = "2024-01-10T08:30:00.000Z";

/// The running pnpm is fresh, so the range falls back to its newest mature
/// version. The running version is exempt from the cutoff everywhere else
/// pnpm resolves itself, which is why the fresh version here is the real
/// `PNPM_VERSION`.
#[tokio::test]
async fn an_immature_running_pnpm_gives_way_to_the_newest_mature_version() {
    let older = older_than_running();
    let fresh = chrono::Utc::now().to_rfc3339();
    let server = serve_pnpm(&[(&older, OLD), (PNPM_VERSION, &fresh)]).await;
    let (config, _cache) = config_with_cutoff(&server, None);

    let version = mature_pnpm_version_for_range(&config, &format!(">={older}"), PNPM_VERSION)
        .await
        .expect("pick a version");

    assert_eq!(version, older);
}

#[tokio::test]
async fn a_mature_running_pnpm_is_recorded_even_when_a_newer_one_is_mature() {
    let server = serve_pnpm(&[("1.0.0", OLD), ("1.1.0", OLD)]).await;
    let (config, _cache) = config_with_cutoff(&server, None);

    let version = mature_pnpm_version_for_range(&config, "^1.0.0", "1.0.0")
        .await
        .expect("pick a version");

    assert_eq!(version, "1.0.0");
}

#[tokio::test]
async fn the_running_pnpm_is_kept_when_nothing_in_the_range_is_mature() {
    let fresh = chrono::Utc::now().to_rfc3339();
    let server = serve_pnpm(&[("1.0.0", OLD), ("2.0.0", &fresh), ("2.1.0", &fresh)]).await;
    let (config, _cache) = config_with_cutoff(&server, None);

    let version = mature_pnpm_version_for_range(&config, "^2.0.0", "2.1.0")
        .await
        .expect("pick a version");

    assert_eq!(version, "2.1.0");
}

#[tokio::test]
async fn an_excluded_running_pnpm_is_recorded() {
    let fresh = chrono::Utc::now().to_rfc3339();
    let server = serve_pnpm(&[("1.0.0", OLD), ("1.1.0", &fresh)]).await;
    let (config, _cache) = config_with_cutoff(&server, Some(vec!["pnpm@1.1.0".to_string()]));

    let version = mature_pnpm_version_for_range(&config, "^1.0.0", "1.1.0")
        .await
        .expect("pick a version");

    assert_eq!(version, "1.1.0");
}

/// The version recorded in place of the running pnpm is one every other
/// contributor installs, so it has to pass the trust policy too.
#[tokio::test]
async fn the_range_fallback_skips_a_trust_downgrade() {
    let fresh = chrono::Utc::now().to_rfc3339();
    let body = serde_json::json!({
        "name": "pnpm",
        "dist-tags": { "latest": "1.2.0" },
        "time": { "1.0.0": OLDER, "1.1.0": OLD, "1.2.0": fresh },
        "versions": {
            "1.0.0": {
                "name": "pnpm",
                "version": "1.0.0",
                "_npmUser": {
                    "name": "alice",
                    "trustedPublisher": { "id": "github", "oidcConfigId": "release" },
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": "https://registry/pnpm-1.0.0.tgz",
                    "attestations": {
                        "provenance": { "predicateType": "https://slsa.dev/provenance/v1" },
                    },
                },
            },
            "1.1.0": {
                "name": "pnpm",
                "version": "1.1.0",
                "dist": {
                    "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                    "tarball": "https://registry/pnpm-1.1.0.tgz",
                },
            },
            "1.2.0": {
                "name": "pnpm",
                "version": "1.2.0",
                "dist": {
                    "integrity": "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==",
                    "tarball": "https://registry/pnpm-1.2.0.tgz",
                },
            },
        },
    });
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/pnpm")
        .with_status(200)
        .with_body(body.to_string())
        .create_async()
        .await;
    let (mut config, _cache) = config_with_cutoff(&server, None);
    config.trust_policy = TrustPolicy::NoDowngrade;

    let version = mature_pnpm_version_for_range(&config, "^1.0.0", "1.2.0")
        .await
        .expect("pick a version");

    assert_eq!(version, "1.0.0");
}

/// Without a cutoff there is nothing to check, so the registry is not asked.
#[tokio::test]
async fn the_running_pnpm_is_recorded_without_a_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let packument = server
        .mock("GET", "/pnpm")
        .expect(0)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let mut config = Config {
        minimum_release_age: Some(0),
        cache_dir: cache_dir.path().to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = format!("{}/", server.url());

    let version = mature_pnpm_version_for_range(&config, "^1.0.0", "1.1.0")
        .await
        .expect("pick a version");

    assert_eq!(version, "1.1.0");
    packument.assert_async().await;
}

fn older_than_running() -> String {
    let running = node_semver::Version::parse(PNPM_VERSION).expect("parse the running version");
    if running.pre_release.is_empty() {
        format!("{}.0.0", running.major - 1)
    } else {
        format!("{}.{}.{}-0", running.major, running.minor, running.patch)
    }
}

fn config_with_cutoff(
    server: &mockito::ServerGuard,
    exclude: Option<Vec<String>>,
) -> (Config, tempfile::TempDir) {
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let mut config = Config {
        minimum_release_age: Some(24 * 60),
        minimum_release_age_exclude: exclude,
        cache_dir: cache_dir.path().to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = format!("{}/", server.url());
    (config, cache_dir)
}

/// A registry serving a `pnpm` packument with `versions`, each published at
/// the paired time. `latest` points at the last one.
async fn serve_pnpm(versions: &[(&str, &str)]) -> mockito::ServerGuard {
    let (latest, _) = versions.last().expect("at least one version");
    let time: serde_json::Map<_, _> = versions
        .iter()
        .map(|(version, published)| ((*version).to_string(), serde_json::json!(published)))
        .collect();
    let manifests: serde_json::Map<_, _> = versions
        .iter()
        .map(|(version, _)| {
            let manifest = serde_json::json!({
                "name": "pnpm",
                "version": version,
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/pnpm-{version}.tgz"),
                },
            });
            ((*version).to_string(), manifest)
        })
        .collect();
    let body = serde_json::json!({
        "name": "pnpm",
        "dist-tags": { "latest": latest },
        "time": time,
        "versions": manifests,
    });
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/pnpm")
        .with_status(200)
        .with_body(body.to_string())
        .create_async()
        .await;
    server
}
