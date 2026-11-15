use super::{
    Arc, DependencyGroup, HashMap, HashSet, Mutex, StubResolver, default_opts, fake_manifest,
    fake_result, peer_context_lockfile, peer_declaring_metadata, plain_dependency,
    snapshot_with_dependencies,
};
use crate::{WorkspaceTreeCtx, resolve_importer::ImporterHoistState};
use serde_json::json;

#[tokio::test]
async fn retained_pins_use_source_qualified_package_manifests() {
    for (specifier, package_id) in [
        ("1.0.0", "consumer@1.0.0(patch_hash=abc)"),
        ("https://example.test/consumer.tgz", "consumer@https://example.test/consumer.tgz"),
        (
            "git+ssh://git@example.test/project/repo#abcdef01",
            "consumer@git+ssh://git@example.test/project/repo#abcdef01",
        ),
    ] {
        assert_manifest_pins(specifier, package_id).await;
    }
}

async fn assert_manifest_pins(specifier: &str, package_id: &str) {
    let suffix = pnpm_lockfile::PeerBindings::Explicit(vec!["provider@1.0.0".to_string()])
        .to_suffix(1)
        .unwrap();
    let version = format!("{}{suffix}", package_id.strip_prefix("consumer@").unwrap());
    let package_key: pnpm_lockfile::PkgNameVerPeer = package_id.parse().unwrap();
    let metadata_key = package_key.without_peer().to_string();
    let mut lockfile = peer_context_lockfile(
        Some((&metadata_key, peer_declaring_metadata(["provider", "ordinary"]))),
        [(
            &format!("consumer@{version}"),
            snapshot_with_dependencies([
                ("provider", plain_dependency("1.0.0")),
                ("ordinary", plain_dependency("1.0.0")),
            ]),
        )],
    );
    lockfile.importers.insert(
        "app".to_string(),
        serde_json::from_value(json!({
            "dependencies": { "consumer": { "specifier": specifier, "version": version } },
        }))
        .unwrap(),
    );
    let workspace =
        Arc::new(WorkspaceTreeCtx::default().with_wanted_lockfile(Some(Arc::new(lockfile))));
    let mut result = fake_result(
        "consumer",
        "1.0.0",
        json!({
            "name": "consumer", "version": "1.0.0",
            "dependencies": { "ordinary": "1.0.0" },
            "peerDependencies": { "provider": "*", "ordinary": "*" },
            "peerDependenciesMeta": { "provider": { "optional": true }, "ordinary": { "optional": true } },
        }),
    );
    result.id = if specifier == "1.0.0" { package_id } else { specifier }.into();
    let resolver = StubResolver {
        table: HashMap::from_iter([
            (("consumer".to_string(), specifier.to_string()), result),
            (
                ("ordinary".to_string(), "1.0.0".to_string()),
                fake_result("ordinary", "1.0.0", json!({"name":"ordinary", "version":"1.0.0"})),
            ),
        ]),
        calls: Mutex::new(Vec::new()),
    };
    let (_directory, manifest) = fake_manifest(json!({"consumer": specifier}));
    let state = ImporterHoistState::init(
        &resolver,
        "app",
        0,
        &manifest,
        [DependencyGroup::Prod],
        default_opts(),
        Arc::clone(&workspace),
    )
    .await
    .unwrap();
    assert!(
        workspace
            .inspect_package(package_id, |_| ())
            .is_some(),
    );
    assert_eq!(
        state.selection.locked_versions.versions(&workspace).as_ref(),
        &HashMap::from_iter([("provider".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

#[tokio::test]
async fn retained_pins_wait_for_the_final_children_owner() {
    let suffix = pnpm_lockfile::PeerBindings::Explicit(vec!["ordinary@2.0.0".to_string()])
        .to_suffix(1)
        .unwrap();
    let version = format!("1.0.0{suffix}");
    let mut lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["ordinary"]))),
        [(
            &format!("consumer@{version}"),
            snapshot_with_dependencies([("ordinary", plain_dependency("2.0.0"))]),
        )],
    );
    lockfile.importers.insert(
        "app".to_string(),
        serde_json::from_value(json!({
            "dependencies": { "consumer": { "specifier": "1.0.0", "version": version } },
        }))
        .unwrap(),
    );
    let workspace =
        Arc::new(WorkspaceTreeCtx::default().with_wanted_lockfile(Some(Arc::new(lockfile))));
    let consumer = fake_result(
        "consumer",
        "1.0.0",
        json!({
            "name": "consumer", "version": "1.0.0", "dependencies": { "ordinary": "1.0.0" },
            "peerDependencies": { "ordinary": "*" }, "peerDependenciesMeta": { "ordinary": { "optional": true } },
        }),
    );
    let resolver = StubResolver {
        table: HashMap::from_iter([
            (("consumer".to_string(), "1.0.0".to_string()), consumer),
            (
                ("ordinary".to_string(), "1.0.0".to_string()),
                fake_result("ordinary", "1.0.0", json!({"name":"ordinary","version":"1.0.0"})),
            ),
            (
                ("ordinary".to_string(), "2.0.0".to_string()),
                fake_result("ordinary", "2.0.0", json!({"name":"ordinary","version":"2.0.0"})),
            ),
        ]),
        calls: Mutex::new(Vec::new()),
    };
    let (_first_directory, first) = fake_manifest(json!({"consumer":"1.0.0", "ordinary":"2.0.0"}));
    let state = ImporterHoistState::init(
        &resolver,
        "app",
        1,
        &first,
        [DependencyGroup::Prod],
        default_opts(),
        Arc::clone(&workspace),
    )
    .await
    .unwrap();
    assert!(
        workspace
            .inspect_package("consumer@1.0.0", |package| package.peer_set
                .taken()
                .contains_key("ordinary"))
            .unwrap(),
    );
    let (_second_directory, second) = fake_manifest(json!({"consumer":"1.0.0"}));
    ImporterHoistState::init(
        &resolver,
        "earlier",
        0,
        &second,
        [DependencyGroup::Prod],
        default_opts(),
        Arc::clone(&workspace),
    )
    .await
    .unwrap();
    assert!(
        !workspace
            .inspect_package("consumer@1.0.0", |package| package.peer_set
                .taken()
                .contains_key("ordinary"))
            .unwrap(),
    );
    assert!(state.selection.locked_versions.versions(&workspace).is_empty());
}
