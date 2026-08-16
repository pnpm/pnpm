use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};

pub(super) fn manifest_result(manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: PkgResolutionId::from("parent@1.0.0"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "parent".to_string(),
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some("parent".to_string()),
        policy_violation: None,
    }
}
