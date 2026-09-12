use super::{
    DependencyGroup, LatestQuery, Mutex, PackageManifest, ResolveDependencyTreeOptions,
    ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
    assert_eq, fake_result, resolve_dependency_tree,
};

#[tokio::test]
async fn passes_optional_flag_to_the_resolver() {
    struct CapturingOptionalResolver {
        optional: Mutex<Vec<Option<bool>>>,
        result: ResolveResult,
    }

    impl Resolver for CapturingOptionalResolver {
        fn resolve<'a>(
            &'a self,
            wanted: &'a WantedDependency,
            _opts: &'a ResolveOptions,
        ) -> ResolveFuture<'a> {
            self.optional.lock().unwrap().push(wanted.optional);
            let result = self.result.clone();
            Box::pin(async move { Ok(Some(result)) })
        }

        fn resolve_latest<'a>(
            &'a self,
            _query: &'a LatestQuery,
            _opts: &'a ResolveOptions,
        ) -> ResolveLatestFuture<'a> {
            Box::pin(async { Ok(None) })
        }
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let manifest_path = temp.path().join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"name":"root","version":"1.0.0","optionalDependencies":{"optional-dep":"1.0.0"}}"#,
    )
    .expect("write manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("parse manifest");
    let resolver = CapturingOptionalResolver {
        optional: Mutex::new(Vec::new()),
        result: fake_result(
            "optional-dep",
            "1.0.0",
            serde_json::json!({ "name": "optional-dep", "version": "1.0.0" }),
        ),
    };

    resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Optional],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("resolve optional dependency");

    assert_eq!(*resolver.optional.lock().unwrap(), vec![Some(true)]);
}
