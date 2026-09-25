use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

use super::{ResolveDependencyTreeOptions, resolve_dependency_tree};

struct NestedWorkspaceLinkResolver {
    target_dir: std::path::PathBuf,
}

impl Resolver for NestedWorkspaceLinkResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let target_dir = self.target_dir.clone();
        let project_dir = opts.project.project_dir.clone();
        let alias = wanted.alias.clone().unwrap_or_default();
        Box::pin(async move {
            if alias != "shared" {
                return Ok(None);
            }
            let relative = pathdiff::diff_paths(target_dir, project_dir)
                .expect("target can be relativized")
                .display()
                .to_string()
                .replace('\\', "/");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(format!("link:{relative}")),
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: relative,
                }),
                resolved_via: "workspace".to_string(),
                normalized_bare_specifier: None,
                alias: Some(alias),
                policy_violation: None,
                package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
                    name_ver: None,
                    latest: None,
                    published_at: None,
                    manifest: Some(std::sync::Arc::new(
                        serde_json::json!({ "name": "shared", "version": "1.0.0" }),
                    )),
                    non_deprecated_alternative: None,
                },
            }))
        })
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
async fn canonical_snapshot_link_id_is_relative_to_lockfile_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest_path = temp.path().join("package.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "shared": "workspace:*" },
        }))
        .expect("serialize manifest"),
    )
    .expect("write manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("parse manifest");
    let project_dir = std::path::PathBuf::from("/repo/apps/nested/app");
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let resolver = NestedWorkspaceLinkResolver { target_dir: lockfile_dir.join("packages/shared") };

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions {
                project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                    project_dir,
                    lockfile_dir,
                    ..Default::default()
                },
                ..ResolveOptions::default()
            },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("resolve nested workspace link");

    let direct = tree.direct.first().expect("shared direct dependency");
    assert_eq!(direct.id, "link:packages/shared");
    assert_eq!(direct.node_id, crate::NodeId::leaf("link:packages/shared"));
    assert!(tree.packages.contains_key("link:packages/shared"));
    assert!(!tree.packages.contains_key("link:../../../packages/shared"));
}

fn acme_no_match_error(
    bare_specifier: &str,
    versions: &[&str],
) -> pnpm_resolving_resolver_base::NoMatchingVersionError {
    pnpm_resolving_resolver_base::NoMatchingVersionError {
        dep: format!("acme@{bare_specifier}"),
        registry: "https://example.com/".to_string(),
        published_versions: String::new(),
        versions: versions
            .iter()
            .map(|v| (*v).to_string())
            .collect(),
    }
}

fn override_entry(
    selector: &str,
    target_name: &str,
    target_range: Option<&str>,
    new_spec: &str,
) -> pnpm_config_parse_overrides::VersionOverride {
    pnpm_config_parse_overrides::VersionOverride {
        selector: selector.to_string(),
        parent_pkg: None,
        target_pkg: pnpm_config_parse_overrides::PackageSelector {
            name: target_name.to_string(),
            bare_specifier: target_range.map(str::to_string),
        },
        new_bare_specifier: new_spec.to_string(),
        converge: false,
    }
}

#[test]
fn override_reframe_names_the_override_and_latest_matching_version() {
    use super::override_reframe::reframe_override_no_matching_version;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.5.0".to_string()),
        ..WantedDependency::default()
    };
    let specific = acme_no_match_error("^1.5.0", &["1.0.0", "1.1.0"]);
    let overrides = vec![override_entry("acme@^1", "acme", Some("^1"), "^1.5.0")];

    let reframed = reframe_override_no_matching_version(&wanted, &specific, Some(&overrides))
        .expect("a matching override reframes the error");

    assert_eq!(
        reframed.to_string(),
        r#"Override "acme@^1": "^1.5.0" targets a version of acme that does not exist on the registry."#,
    );
    assert_eq!(
        miette::Diagnostic::code(&reframed).map(|c| c.to_string()),
        Some("ERR_PNPM_NO_MATCHING_VERSION".to_string()),
    );
    assert_eq!(
        miette::Diagnostic::help(&reframed).map(|h| h.to_string()),
        Some(r#"The latest release of acme matching "^1" is "1.1.0"."#.to_string()),
    );
}

#[test]
fn override_reframe_omits_help_when_selector_has_no_range() {
    use super::override_reframe::reframe_override_no_matching_version;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let specific = acme_no_match_error("2.0.0", &["1.0.0", "1.1.0"]);
    let overrides = vec![override_entry("acme", "acme", None, "2.0.0")];

    let reframed = reframe_override_no_matching_version(&wanted, &specific, Some(&overrides))
        .expect("a matching override reframes the error");

    assert_eq!(
        reframed.to_string(),
        r#"Override "acme": "2.0.0" targets a version of acme that does not exist on the registry."#,
    );
    assert!(miette::Diagnostic::help(&reframed).is_none());
}

#[test]
fn override_reframe_returns_none_when_no_override_matches() {
    use super::override_reframe::reframe_override_no_matching_version;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.5.0".to_string()),
        ..WantedDependency::default()
    };
    let specific = acme_no_match_error("^1.5.0", &["1.0.0", "1.1.0"]);
    let overrides = vec![override_entry("acme@^1", "acme", Some("^1"), "^3.0.0")];

    assert!(reframe_override_no_matching_version(&wanted, &specific, Some(&overrides)).is_none());
}

#[test]
fn max_satisfying_picks_the_highest_version_in_range() {
    use super::override_reframe::max_satisfying;

    let versions = ["1.0.0".to_string(), "1.1.0".to_string(), "2.0.0".to_string()];
    assert_eq!(max_satisfying(&versions, "^1"), Some("1.1.0".to_string()));
    assert_eq!(max_satisfying(&versions, "^2"), Some("2.0.0".to_string()));
    assert_eq!(max_satisfying(&versions, "^3"), None);
}
