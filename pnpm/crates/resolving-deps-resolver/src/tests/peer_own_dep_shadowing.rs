use super::{
    DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions, StubResolver,
    fake_manifest, fake_result, resolve_dependency_tree,
};

fn opts(auto_install_peers: bool) -> ResolveDependencyTreeOptions {
    ResolveDependencyTreeOptions {
        base_opts: ResolveOptions::default(),
        patched_dependencies: None,
        manifest_hook: None,
        overrides_hook: None,
        pnpmfile_hook: None,
        read_package_log: None,
        auto_install_peers,
    }
}

fn parser_table() -> HashMap<(String, String), pnpm_resolving_resolver_base::ResolveResult> {
    let mut table = HashMap::default();
    table.insert(
        ("parser".to_string(), "^1.0.0".to_string()),
        fake_result(
            "parser",
            "1.0.0",
            serde_json::json!({
                "name": "parser",
                "version": "1.0.0",
                "dependencies": { "types": "^1.0.0" },
                "peerDependencies": { "types": "*" },
            }),
        ),
    );
    table.insert(
        ("types".to_string(), "^1.0.0".to_string()),
        fake_result("types", "1.0.0", serde_json::json!({ "name": "types", "version": "1.0.0" })),
    );
    table
}

/// [`parser_table`] plus the higher `types` an importer can hold as
/// a direct dependency of its own.
fn shadowing_table() -> HashMap<(String, String), pnpm_resolving_resolver_base::ResolveResult> {
    let mut table = parser_table();
    table.insert(
        ("types".to_string(), "^2.0.0".to_string()),
        fake_result("types", "2.0.0", serde_json::json!({ "name": "types", "version": "2.0.0" })),
    );
    table
}

#[tokio::test]
async fn auto_install_peers_keeps_the_peer_and_drops_the_own_dep() {
    let resolver = StubResolver { table: parser_table(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "parser": "^1.0.0" }));

    let tree = resolve_dependency_tree(&resolver, &manifest, [DependencyGroup::Prod], opts(true))
        .await
        .unwrap();

    let parser = tree.packages.get("parser@1.0.0").expect("parser resolved");
    assert!(
        parser.peer_dependencies.contains_key("types"),
        "the peer survives when it also appears in the package's own dependencies",
    );
    assert!(
        !tree.packages.contains_key("types@1.0.0"),
        "the peer-shadowed own dependency is not walked as a child",
    );
}

#[tokio::test]
async fn without_auto_install_peers_the_own_dep_wins() {
    let resolver = StubResolver { table: parser_table(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "parser": "^1.0.0" }));

    let tree = resolve_dependency_tree(&resolver, &manifest, [DependencyGroup::Prod], opts(false))
        .await
        .unwrap();

    let parser = tree.packages.get("parser@1.0.0").expect("parser resolved");
    assert!(
        !parser.peer_dependencies.contains_key("types"),
        "the peer is dropped when the package supplies the name itself",
    );
    assert!(tree.packages.contains_key("types@1.0.0"), "the own dependency is walked");
}

/// `types` is a direct dependency of the importer, so it is in
/// `parser`'s parent scope and the peer edge resolves against it
/// instead of `parser` nesting its own copy — pnpm's
/// `parentPkgAliases` arm of the omission, which applies with
/// `autoInstallPeers` off.
#[tokio::test]
async fn a_peer_in_the_parent_scope_shadows_the_own_dep() {
    let resolver = StubResolver { table: shadowing_table(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "parser": "^1.0.0", "types": "^2.0.0" }));

    let tree = resolve_dependency_tree(&resolver, &manifest, [DependencyGroup::Prod], opts(false))
        .await
        .unwrap();

    let parser = tree.packages.get("parser@1.0.0").expect("parser resolved");
    assert!(
        parser.peer_dependencies.contains_key("types"),
        "the peer survives when the parent scope already supplies the name",
    );
    assert!(
        !tree.packages.contains_key("types@1.0.0"),
        "the shadowed own dependency is not walked as a child",
    );
    assert!(tree.packages.contains_key("types@2.0.0"), "the parent's copy is the one resolved");
}

/// The scope accumulates level by level: `types` is the importer's
/// direct dependency and `parser` sits two levels below it.
#[tokio::test]
async fn the_parent_scope_reaches_every_level_below_it() {
    let mut table = shadowing_table();
    table.insert(
        ("wrapper".to_string(), "^1.0.0".to_string()),
        fake_result(
            "wrapper",
            "1.0.0",
            serde_json::json!({
                "name": "wrapper",
                "version": "1.0.0",
                "dependencies": { "parser": "^1.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "wrapper": "^1.0.0", "types": "^2.0.0" }));

    let tree = resolve_dependency_tree(&resolver, &manifest, [DependencyGroup::Prod], opts(false))
        .await
        .unwrap();

    let parser = tree.packages.get("parser@1.0.0").expect("parser resolved");
    assert!(parser.peer_dependencies.contains_key("types"));
    assert!(!tree.packages.contains_key("types@1.0.0"));
    assert!(tree.packages.contains_key("types@2.0.0"), "the importer's copy is the one resolved");
}

#[tokio::test]
async fn non_optional_meta_only_entry_is_not_a_peer() {
    let mut table = HashMap::default();
    table.insert(
        ("pkg".to_string(), "^1.0.0".to_string()),
        fake_result(
            "pkg",
            "1.0.0",
            serde_json::json!({
                "name": "pkg",
                "version": "1.0.0",
                "peerDependenciesMeta": {
                    "ghost": {},
                    "wanted-optional": { "optional": true },
                },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "pkg": "^1.0.0" }));

    let tree = resolve_dependency_tree(&resolver, &manifest, [DependencyGroup::Prod], opts(false))
        .await
        .unwrap();

    let pkg = tree.packages.get("pkg@1.0.0").expect("pkg resolved");
    assert!(
        !pkg.peer_dependencies.contains_key("ghost"),
        "a peerDependenciesMeta entry without optional: true and without a peerDependencies entry is ignored",
    );
    let optional_peer =
        pkg.peer_dependencies.get("wanted-optional").expect("optional meta-only peer kept");
    assert!(optional_peer.optional);
    assert_eq!(optional_peer.version, "*");
}
