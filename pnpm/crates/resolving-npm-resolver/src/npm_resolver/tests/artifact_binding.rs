use super::{
    LockfileResolution, MalformedRevisionHistoryError, ResolveOptions, TarballRevision,
    WantedDependency, assert_eq, build_resolver, json, revision_history_package_body,
    revision_package_body, shasum_only_package_body,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn explicit_revision_selects_its_artifact_and_manifest() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r1".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions { calc_specifier: true, ..ResolveOptions::default() };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(1));
    assert!(resolution.tarball.ends_with("sha512/Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g"));
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("1.0.0+r1"));
    let manifest = result.manifest.as_ref().expect("manifest");
    assert_eq!(manifest["name"], "acme");
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["deprecated"], "current warning");
    assert_eq!(manifest["dependencies"], json!({ "fixed": "1.0.0" }));
    assert!(manifest.get("optionalDependencies").is_none());
}

#[tokio::test]
async fn revision_metadata_rejects_a_tarball_from_another_registry() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("https://attacker.example/-/tarballs/sha512/{}", "A".repeat(86));
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_package_body(&tarball, &json!(1)))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("a revision URL from another registry must fail the resolve");

    assert!(error.downcast_ref::<MalformedRevisionHistoryError>().is_some());
}

#[tokio::test]
async fn shasum_only_metadata_resolves_to_a_sha1_integrity() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(shasum_only_package_body("e21bf1d18b7ce29d1cd45f6d8e0e8bcd0a4ca8ba"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();

    let LockfileResolution::Tarball(tarball) = &result.resolution else {
        panic!("expected a tarball resolution, got {:?}", result.resolution);
    };
    assert_eq!(
        tarball.integrity.as_ref().map(ToString::to_string).as_deref(),
        Some("sha1-4hvx0Yt84p0c1F9tjg6LzQpMqLo="),
    );
}
