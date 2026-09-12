use super::{
    DependencyGroup, HashMap, Mutex, ResolveDependencyTreeError, ResolveImporterError,
    ResolveImporterOptions, StubResolver, assert_eq, default_opts, fake_manifest, fake_result,
    resolve_importer,
};

/// `catalog:` on a direct dependency is rewritten to the catalog's
/// recorded specifier before the resolver chain sees the wanted dep.
/// The dereference is importer-only.
#[tokio::test]
async fn catalog_protocol_on_direct_dep_is_rewritten() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "catalog:" }));

    let mut catalogs = pnpm_catalogs_types::Catalogs::new();
    catalogs.insert(
        "default".to_string(),
        std::iter::once(("foo".to_string(), "^1.0.0".to_string())).collect(),
    );

    let opts = ResolveImporterOptions { catalogs, ..default_opts() };
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();
    assert_eq!(result.resolved_tree.direct.len(), 1);
    assert_eq!(result.resolved_tree.direct[0].alias, "foo");
    let calls = resolver.calls.lock().unwrap();
    assert_eq!(&*calls, &[("foo".to_string(), "^1.0.0".to_string())]);
}

/// A misconfigured `catalog:` entry (here: missing alias) short-
/// circuits resolution with the `CATALOG_ENTRY_NOT_FOUND_FOR_SPEC`
/// error rather than falling through to `SpecNotSupported`.
#[tokio::test]
async fn catalog_misconfiguration_surfaces_pnpm_error_code() {
    let resolver = StubResolver { table: HashMap::default(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "catalog:" }));

    let err = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .expect_err("missing catalog entry must error");
    match err {
        ResolveImporterError::Resolve(ResolveDependencyTreeError::CatalogMisconfiguration(
            inner,
        )) => {
            assert_eq!(
                inner.to_string(),
                "No catalog entry 'foo' was found for catalog 'default'.",
            );
        }
        other => panic!("expected CatalogMisconfiguration, got {other:?}"),
    }
}
