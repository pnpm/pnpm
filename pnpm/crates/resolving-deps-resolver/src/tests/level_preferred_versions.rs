use super::{HashMap, Mutex, fake_manifest, fake_result};
use crate::resolve_dependency_tree::{ResolveDependencyTreeOptions, resolve_dependency_tree};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

/// Stub that records, per `(alias, range)`, the overlay's preferred
/// versions for the probe package `pinned` at resolve time — the
/// name a real picker would merge the overlay under.
struct OverlayRecordingResolver {
    table: HashMap<(String, String), ResolveResult>,
    seen_overlay: Mutex<HashMap<(String, String), Vec<String>>>,
}

impl Resolver for OverlayRecordingResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let name = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        let overlay_view: Vec<String> = opts
            .preferred_versions_overlay
            .as_ref()
            .map(|overlay| overlay.versions_for("pinned").into_iter().map(str::to_string).collect())
            .unwrap_or_default();
        self.seen_overlay.lock().unwrap().insert((name.clone(), range.clone()), overlay_view);
        let result = self.table.get(&(name, range)).cloned();
        Box::pin(async move { Ok::<_, ResolveError>(result) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
async fn child_resolution_prefers_parent_level_sibling_versions() {
    let mut table = HashMap::default();
    table.insert(
        ("parent".to_string(), "1.0.0".to_string()),
        fake_result(
            "parent",
            "1.0.0",
            serde_json::json!({
                "name": "parent",
                "version": "1.0.0",
                "dependencies": { "consumer": "1.0.0", "pinned": "5.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer",
            "1.0.0",
            serde_json::json!({
                "name": "consumer",
                "version": "1.0.0",
                "dependencies": { "pinned": "~5.0.0" },
            }),
        ),
    );
    for range in ["5.0.0", "~5.0.0"] {
        table.insert(
            ("pinned".to_string(), range.to_string()),
            fake_result(
                "pinned",
                "5.0.0",
                serde_json::json!({ "name": "pinned", "version": "5.0.0" }),
            ),
        );
    }
    let resolver = OverlayRecordingResolver { table, seen_overlay: Mutex::new(HashMap::default()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "parent": "1.0.0" }));

    resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
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
    .unwrap();

    let seen = resolver.seen_overlay.lock().unwrap();
    assert_eq!(
        seen.get(&("parent".to_string(), "1.0.0".to_string())),
        Some(&Vec::new()),
        "importer-level direct deps resolve with no overlay",
    );
    assert_eq!(
        seen.get(&("pinned".to_string(), "~5.0.0".to_string())),
        Some(&vec!["5.0.0".to_string()]),
        "the consumer's child sees its parent-level sibling's resolved version",
    );
}

#[tokio::test]
async fn npm_alias_child_consults_overlay_by_inner_name() {
    let mut table = HashMap::default();
    table.insert(
        ("parent".to_string(), "1.0.0".to_string()),
        fake_result(
            "parent",
            "1.0.0",
            serde_json::json!({
                "name": "parent",
                "version": "1.0.0",
                "dependencies": { "consumer": "1.0.0", "pinned": "5.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer",
            "1.0.0",
            serde_json::json!({
                "name": "consumer",
                "version": "1.0.0",
                "dependencies": { "renamed": "npm:pinned@~5.0.0" },
            }),
        ),
    );
    table.insert(
        ("pinned".to_string(), "5.0.0".to_string()),
        fake_result("pinned", "5.0.0", serde_json::json!({ "name": "pinned", "version": "5.0.0" })),
    );
    table.insert(
        ("renamed".to_string(), "npm:pinned@~5.0.0".to_string()),
        fake_result("pinned", "5.0.0", serde_json::json!({ "name": "pinned", "version": "5.0.0" })),
    );
    let resolver = OverlayRecordingResolver { table, seen_overlay: Mutex::new(HashMap::default()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "parent": "1.0.0" }));

    resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
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
    .unwrap();

    let seen = resolver.seen_overlay.lock().unwrap();
    assert_eq!(
        seen.get(&("renamed".to_string(), "npm:pinned@~5.0.0".to_string())),
        Some(&vec!["5.0.0".to_string()]),
        "the npm: alias edge must carry the overlay so the picker can merge by the inner name",
    );
}
