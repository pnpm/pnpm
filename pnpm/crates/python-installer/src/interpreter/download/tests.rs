use super::{
    Bounds, Releases, ShasumsFileItem, Source, builds_in, exact_version, host_triple,
    releases::read_cached_release_tags, within,
};
use crate::interpreter::VersionRequest;
use pnpm_config::{Config, Tool, ToolSettings};
use pnpm_network::ThrottledClient;

/// A python-build-standalone `SHA256SUMS`, as the release writes it and
/// as the shared parser hands it back.
fn sums(files: &[String]) -> Vec<ShasumsFileItem> {
    use std::fmt::Write as _;
    let mut index = String::new();
    for file in files {
        writeln!(index, "{}  {file}", "a".repeat(64)).expect("writing to a String cannot fail");
    }
    pnpm_crypto_shasums_file::parse_shasums_file(&index)
}

fn built(versions: &[&str], triple: &str) -> Vec<String> {
    versions
        .iter()
        .map(|version| format!("cpython-{version}+20260901-{triple}-install_only_stripped.tar.gz"))
        .collect()
}

fn requires(specifiers: &str) -> pep440_rs::VersionSpecifiers {
    specifiers.parse().expect("requires-python fixture")
}

#[tokio::test]
async fn release_tag_cache_obeys_its_freshness_window() {
    let cache = tempfile::NamedTempFile::new().expect("release tag cache");
    tokio::fs::write(cache.path(), r#"["20260101"]"#).await.expect("write release tag cache");

    assert!(read_cached_release_tags(cache.path(), std::time::Duration::ZERO).await.is_none());
    assert_eq!(
        read_cached_release_tags(cache.path(), std::time::Duration::from_mins(1)).await,
        Some(vec!["20260101".to_string()]),
    );
}

#[test]
fn the_index_offers_the_ordinary_interpreter_of_this_machine() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut files = built(&["3.13.15", "3.14.7"], &triple);
    // Other platforms, other builds of the same version, and the variants
    // a project asks for by name rather than by version.
    files.push(
        "cpython-3.13.15+20260901-riscv64-unknown-linux-gnu-install_only_stripped.tar.gz"
            .to_string(),
    );
    files.push(format!("cpython-3.13.15+20260901-{triple}-install_only.tar.gz"));
    files.push(format!("cpython-3.13.15+20260901-{triple}-debug-full.tar.zst"));
    files.extend(built(&["3.13.15"], &format!("{triple}-freethreaded")));
    let offered = builds_in(&sums(&files))
        .iter()
        .map(|build| build.version.to_string())
        .collect::<Vec<_>>();
    dbg!(&offered);
    assert_eq!(offered, ["3.13.15", "3.14.7"]);
}

/// A release index names what pnpm downloads and where it puts it.
#[test]
fn an_index_naming_a_path_rather_than_a_build_offers_nothing() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    for named in [
        "cpython-3.13.15+../../../elsewhere",
        "cpython-3.13.15+tag/../..",
        "cpython-3.13.15+",
        "cpython-3.13.15+2026-09-01",
    ] {
        let index = sums(&[format!("{named}-{triple}-install_only_stripped.tar.gz")]);
        assert!(builds_in(&index).is_empty(), "{named}");
    }
    let short = pnpm_crypto_shasums_file::parse_shasums_file(&format!(
        "aa  cpython-3.13.15+20260901-{triple}-install_only_stripped.tar.gz\n",
    ));
    assert!(builds_in(&short).is_empty());
}

#[test]
fn the_build_installed_is_the_newest_one_the_project_accepts() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let files = built(&["3.11.16", "3.12.14", "3.13.15", "3.14.7"], &triple);
    let releases = Releases { builds: builds_in(&sums(&files)) };

    assert_eq!(
        releases
            .best(None, None)
            .expect("the newest build")
            .version()
            .to_string(),
        "3.14.7",
    );
    let accepted = releases
        .best(Some(&requires(">=3.11,<3.14")), None)
        .expect("the newest build the range accepts");
    assert_eq!(accepted.version().to_string(), "3.13.15");
    assert_eq!(accepted.file, files[2]);
    assert_eq!(accepted.tag, "20260901");
    assert_eq!(accepted.integrity.to_string(), sums(&files)[2].integrity);
    assert_eq!(
        releases
            .best(None, Some(&VersionRequest::asking_for(&[3, 12])))
            .expect("the version the pin asks for")
            .version()
            .to_string(),
        "3.12.14",
    );
    assert!(
        releases
            .best(Some(&requires("==3.9.1")), None)
            .is_none(),
    );
}

#[test]
fn an_exact_patch_pin_names_the_historical_version_to_find() {
    assert_eq!(
        exact_version(Some(&requires("==3.13.13")), None)
            .expect("an exact requires-python")
            .to_string(),
        "3.13.13",
    );
    assert_eq!(
        exact_version(None, Some(&VersionRequest::asking_for(&[3, 13, 13])))
            .expect("an exact .python-version request")
            .to_string(),
        "3.13.13",
    );
    assert!(exact_version(None, Some(&VersionRequest::asking_for(&[3, 13]))).is_none());
    assert_eq!(
        exact_version(
            Some(&requires("==3.13.13")),
            Some(&VersionRequest::asking_for(&[3, 12, 12])),
        )
        .expect("the project's exact requirement outranks an incompatible request")
        .to_string(),
        "3.13.13",
    );
}

#[tokio::test]
async fn an_exact_patch_pin_reads_the_release_that_published_it() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let pinned_file = format!("cpython-3.13.13+20260602-{triple}-install_only_stripped.tar.gz");
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_body(r#"[{"name":"20260501"},{"name":"20260901"},{"name":"20260602"}]"#)
        .expect(1)
        .create_async()
        .await;
    let pinned = server
        .mock("GET", "/download/20260602/SHA256SUMS")
        .with_body(format!("{}  {pinned_file}\n", "b".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let cache = tempfile::tempdir().expect("cache directory");
    let mut config = Config::new();
    config.cache_dir = cache.path().to_path_buf();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    for _ in 0..2 {
        let releases = Releases::read_from(
            &config,
            &ThrottledClient::new_for_installs(),
            Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
            Some(&exact),
        )
        .await
        .expect("read the historical release");

        assert_eq!(
            releases
                .best(Some(&requires("==3.13.13")), None)
                .expect("the pinned build")
                .file,
            pinned_file,
        );
    }
    latest.assert_async().await;
    tags.assert_async().await;
    pinned.assert_async().await;
}

#[tokio::test]
async fn a_cached_tag_list_refreshes_after_a_historical_miss() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let initial_tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::Any)
        .with_body(r#"[{"name":"20260601"}]"#)
        .expect(1)
        .create_async()
        .await;
    let old_file = built(&["3.13.12"], &triple).remove(0);
    let old = server
        .mock("GET", "/download/20260601/SHA256SUMS")
        .with_body(format!("{}  {old_file}\n", "b".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let cache = tempfile::tempdir().expect("cache directory");
    let mut config = Config::new();
    config.cache_dir = cache.path().to_path_buf();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await
    .expect("read the initial historical releases");
    assert!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .is_none(),
    );
    initial_tags.assert_async().await;
    initial_tags.remove_async().await;

    let refreshed_tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::Any)
        .with_body(r#"[{"name":"20260602"},{"name":"20260601"}]"#)
        .expect(1)
        .create_async()
        .await;
    let pinned_file = built(&["3.13.13"], &triple).remove(0);
    let pinned = server
        .mock("GET", "/download/20260602/SHA256SUMS")
        .with_body(format!("{}  {pinned_file}\n", "c".repeat(64)))
        .expect(1)
        .create_async()
        .await;

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await
    .expect("refresh the historical release tags");

    assert_eq!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .expect("the newly published pinned build")
            .file,
        pinned_file,
    );
    latest.assert_async().await;
    old.assert_async().await;
    refreshed_tags.assert_async().await;
    pinned.assert_async().await;
}

#[tokio::test]
async fn historical_manifests_honor_the_configured_retry_policy() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::Any)
        .with_body(r#"[{"name":"20260602"}]"#)
        .expect(1)
        .create_async()
        .await;
    let historical = server
        .mock("GET", "/download/20260602/SHA256SUMS")
        .with_status(500)
        .expect(3)
        .create_async()
        .await;
    let cache = tempfile::tempdir().expect("cache directory");
    let mut config = Config::new();
    config.cache_dir = cache.path().to_path_buf();
    config.fetch_retries = 2;
    config.fetch_retry_mintimeout = 0;
    config.fetch_retry_maxtimeout = 0;
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let result = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await;
    result.err().expect("permanent failures exhaust the configured retry budget");

    latest.assert_async().await;
    tags.assert_async().await;
    historical.assert_async().await;
}

#[tokio::test]
async fn an_exact_patch_pin_checks_neighboring_releases_for_this_machine() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let other_triple = if triple == "x86_64-pc-windows-msvc" {
        "aarch64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let other_file =
        format!("cpython-3.13.13+20260501-{other_triple}-install_only_stripped.tar.gz");
    let pinned_file = format!("cpython-3.13.13+20260602-{triple}-install_only_stripped.tar.gz");
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_body(
            r#"[{"name":"20260401"},{"name":"20260901"},{"name":"20260501"},{"name":"20260602"}]"#,
        )
        .expect(1)
        .create_async()
        .await;
    let other = server
        .mock("GET", "/download/20260501/SHA256SUMS")
        .with_body(format!("{}  {other_file}\n", "b".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let pinned = server
        .mock("GET", "/download/20260602/SHA256SUMS")
        .with_body(format!("{}  {pinned_file}\n", "c".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let config = Config::new();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await
    .expect("read the neighboring historical release");

    assert_eq!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .expect("the pinned build")
            .file,
        pinned_file,
    );
    latest.assert_async().await;
    tags.assert_async().await;
    other.assert_async().await;
    pinned.assert_async().await;
}

#[tokio::test]
async fn other_machines_do_not_choose_the_historical_search_direction() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let other_triple = if triple == "x86_64-pc-windows-msvc" {
        "aarch64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let midpoint_file = format!("cpython-3.13.12+20260501-{triple}-install_only_stripped.tar.gz");
    let other_file =
        format!("cpython-3.13.14+20260501-{other_triple}-install_only_stripped.tar.gz");
    let pinned_file = format!("cpython-3.13.13+20260602-{triple}-install_only_stripped.tar.gz");
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_body(
            r#"[{"name":"20260401"},{"name":"20260901"},{"name":"20260501"},{"name":"20260602"}]"#,
        )
        .expect(1)
        .create_async()
        .await;
    let midpoint = server
        .mock("GET", "/download/20260501/SHA256SUMS")
        .with_body(format!(
            "{}  {midpoint_file}\n{}  {other_file}\n",
            "b".repeat(64),
            "c".repeat(64),
        ))
        .expect(1)
        .create_async()
        .await;
    let pinned = server
        .mock("GET", "/download/20260602/SHA256SUMS")
        .with_body(format!("{}  {pinned_file}\n", "d".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let config = Config::new();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await
    .expect("read the historical release for this machine");

    assert_eq!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .expect("the pinned build")
            .file,
        pinned_file,
    );
    latest.assert_async().await;
    tags.assert_async().await;
    midpoint.assert_async().await;
    pinned.assert_async().await;
}

#[tokio::test]
async fn a_release_without_this_machine_does_not_end_the_historical_search() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let other_triple = if triple == "x86_64-pc-windows-msvc" {
        "aarch64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let gap_file = format!("cpython-3.13.13+20260701-{other_triple}-install_only_stripped.tar.gz");
    let nearer_file = format!("cpython-3.13.12+20260801-{triple}-install_only_stripped.tar.gz");
    let pinned_file = format!("cpython-3.13.13+20260901-{triple}-install_only_stripped.tar.gz");
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_body(
            r#"[{"name":"20260501"},{"name":"20260701"},{"name":"20260901"},{"name":"20260601"},{"name":"20260801"}]"#,
        )
        .expect(1)
        .create_async()
        .await;
    let gap = server
        .mock("GET", "/download/20260701/SHA256SUMS")
        .with_body(format!("{}  {gap_file}\n", "b".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let nearer = server
        .mock("GET", "/download/20260801/SHA256SUMS")
        .with_body(format!("{}  {nearer_file}\n", "c".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let pinned = server
        .mock("GET", "/download/20260901/SHA256SUMS")
        .with_body(format!("{}  {pinned_file}\n", "d".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let config = Config::new();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await
    .expect("search past a release without a build for this machine");

    assert_eq!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .expect("the pinned build")
            .file,
        pinned_file,
    );
    latest.assert_async().await;
    tags.assert_async().await;
    gap.assert_async().await;
    nearer.assert_async().await;
    pinned.assert_async().await;
}

#[tokio::test]
async fn exhausting_the_host_gap_budget_is_not_reported_as_absence() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let other_triple = if triple == "x86_64-pc-windows-msvc" {
        "aarch64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let release_tags = [
        "20260111", "20260110", "20260109", "20260108", "20260107", "20260106", "20260105",
        "20260104", "20260103", "20260102", "20260101",
    ];
    let tags = server
        .mock("GET", "/tags")
        .match_query(mockito::Matcher::Any)
        .with_body(
            serde_json::to_string(
                &release_tags
                    .iter()
                    .map(|tag| serde_json::json!({ "name": tag }))
                    .collect::<Vec<_>>(),
            )
            .expect("serialize release tags"),
        )
        .expect(1)
        .create_async()
        .await;
    let mut manifests = Vec::new();
    for tag in [
        "20260106", "20260109", "20260103", "20260110", "20260107", "20260104", "20260101",
        "20260111", "20260108",
    ] {
        let file = format!("cpython-3.13.13+{tag}-{other_triple}-install_only_stripped.tar.gz");
        manifests.push(
            server
                .mock("GET", format!("/download/{tag}/SHA256SUMS").as_str())
                .with_body(format!("{}  {file}\n", "b".repeat(64)))
                .expect(1)
                .create_async()
                .await,
        );
    }
    let farther_file = format!("cpython-3.13.13+20260102-{triple}-install_only_stripped.tar.gz");
    let farther = server
        .mock("GET", "/download/20260102/SHA256SUMS")
        .with_body(format!("{}  {farther_file}\n", "c".repeat(64)))
        .expect(0)
        .create_async()
        .await;
    let cache = tempfile::tempdir().expect("cache directory");
    let mut config = Config::new();
    config.cache_dir = cache.path().to_path_buf();
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let result = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::from_urls(&server.url(), &format!("{}/tags", server.url())),
        Some(&exact),
    )
    .await;
    let Err(error) = result else { panic!("an inconclusive bounded lookup must be reported") };

    assert_eq!(
        error
            .code()
            .expect("lookup error carries a code")
            .to_string(),
        "ERR_PNPM_PYTHON_RELEASE_LOOKUP_LIMIT",
    );
    latest.assert_async().await;
    tags.assert_async().await;
    for manifest in manifests {
        manifest.assert_async().await;
    }
    farther.assert_async().await;
}

#[tokio::test]
async fn a_mirror_does_not_require_the_upstream_release_list() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut server = mockito::Server::new_async().await;
    let latest_file = built(&["3.13.15"], &triple).remove(0);
    let latest = server
        .mock("GET", "/latest/download/SHA256SUMS")
        .with_body(format!("{}  {latest_file}\n", "a".repeat(64)))
        .expect(1)
        .create_async()
        .await;
    let mut config = Config::new();
    config.tools.insert(
        Tool::Python,
        ToolSettings { mirror: Some(server.url()), ..ToolSettings::default() },
    );
    let exact: pep440_rs::Version = "3.13.13".parse().expect("version fixture");

    let releases = Releases::read_from(
        &config,
        &ThrottledClient::new_for_installs(),
        Source::configured(&config),
        Some(&exact),
    )
    .await
    .expect("read the mirror's latest release");

    assert!(
        releases
            .best(Some(&requires("==3.13.13")), None)
            .is_none(),
    );
    latest.assert_async().await;
}

/// What an archive holds is what it expands to and how many files that
/// is, neither of which is what a mirror had to send to hold it.
#[test]
fn an_archive_past_what_an_interpreter_is_never_reaches_the_store() {
    let archive = tempfile::NamedTempFile::new().expect("an archive");
    let mut written =
        tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for entry in 0..4 {
        let file = "an interpreter".repeat(8);
        let mut header = tar::Header::new_gnu();
        header.set_size(file.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        written
            .append_data(&mut header, format!("python/lib/{entry}"), file.as_bytes())
            .expect("an entry");
    }
    let compressed = written
        .into_inner()
        .expect("the archive")
        .finish()
        .expect("the archive");
    std::fs::write(archive.path(), &compressed).expect("write the archive");

    within(archive.path(), Bounds { bytes: 64 * 1024, entries: 4 })
        .expect("an archive within both bounds");

    let past_bytes = within(archive.path(), Bounds { bytes: 64, entries: 4 })
        .expect_err("an archive past the bytes it may unpack to");
    assert!(format!("{past_bytes:?}").contains("unpacks to more than"), "{past_bytes:?}");

    let past_entries = within(archive.path(), Bounds { bytes: 64 * 1024, entries: 3 })
        .expect_err("an archive past the entries it may hold");
    assert!(format!("{past_entries:?}").contains("more than 3 entries"), "{past_entries:?}");
}
