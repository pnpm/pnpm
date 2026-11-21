pub(super) use matching::{
    extract_local_package_target,
    importer_specifier_declares_local,
    parent_semver_or_version,
    policy_authorizes_importer_dep,
    policy_authorizes_link,
    policy_authorizes_package,
    policy_authorizes_peer_spec,
    target_escapes_workspace,
};

use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    path::Path,
    sync::Arc,
};

use parse::{
    build_spec_targets,
    deref_catalog,
    parse_extension_edge,
    parse_override_edge,
    relative_posix_path,
};
use pnpm_lockfile::PkgNameVerPeer;

mod matching;
mod parse;

/// Dependency manifest field in which a child or peer was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LocalDepField {
    Dependencies,
    OptionalDependencies,
    PeerDependencies,
}

/// Map of `(parent_base_pkg_key, dep_field, child_alias)` to the pristine range
/// declared by that non-local package source before hooks or overrides.
pub type PristineDeclaredSpecs = HashMap<(PkgNameVerPeer, LocalDepField, String), String>;

/// Workspace catalog table (`catalog_name -> pkg_name -> range`).
pub type PolicyCatalogMap = BTreeMap<String, BTreeMap<String, String>>;

/// One configured workspace override, `packageExtension`, or probed
/// `readPackage` rewrite that authorizes a local filesystem dependency edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedLocalEdge {
    pub parent_pkg_key: Option<PkgNameVerPeer>,
    pub parent_name: Option<String>,
    pub parent_range: Option<String>,
    pub child_alias: String,
    pub child_range: Option<String>,
    pub targets: Vec<String>,
}

/// Workspace and lockfile directory boundaries for offline containment checks.
#[derive(Debug, Clone)]
pub struct WorkspaceContainmentScope {
    pub allow_deep_workspace_links: bool,
    pub workspace_depth_above_lockfile: Option<usize>,
    pub workspace_dir: Option<Arc<Path>>,
    pub lockfile_dir: Option<Arc<Path>>,
    pub overrides_root_dir: Option<Arc<Path>>,
    pub live_workspace_dirs: Option<Arc<[String]>>,
    pub selected_importer_ids: Option<Arc<[String]>>,
}

impl Default for WorkspaceContainmentScope {
    fn default() -> Self {
        Self {
            allow_deep_workspace_links: false,
            workspace_depth_above_lockfile: Some(0),
            workspace_dir: None,
            lockfile_dir: None,
            overrides_root_dir: None,
            live_workspace_dirs: None,
            selected_importer_ids: None,
        }
    }
}

/// Active workspace policy governing whether non-importer snapshots may
/// reference local (`directory`, `file:`, `link:`, `git+file:`) dependencies.
#[derive(Debug, Default, Clone)]
pub struct LocalResolutionPolicy {
    pub authorized_edges: Option<Arc<[AuthorizedLocalEdge]>>,
    pub pristine_declared_specs: Option<Arc<PristineDeclaredSpecs>>,
    pub catalogs: Option<Arc<PolicyCatalogMap>>,
    pub has_after_all_resolved_hook: bool,
    pub allow_deploy_file_url_keys: bool,
    pub scope: WorkspaceContainmentScope,
}

impl LocalResolutionPolicy {
    /// Build a policy from `(selector, spec)` override pairs,
    /// `(parent_selector, child_alias, spec)` extension triples, and pnpmfile
    /// hook presence.
    #[must_use]
    pub fn from_scoped_inputs<'a>(
        overrides: impl IntoIterator<Item = (&'a str, &'a str)>,
        extensions: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
        has_pnpmfile_hook: bool,
        lockfile_dir: Option<&Path>,
    ) -> Self {
        Self::from_scoped_inputs_with_catalog(
            overrides,
            extensions,
            has_pnpmfile_hook,
            lockfile_dir,
            |_, _| None,
        )
    }

    /// Like [`Self::from_scoped_inputs`], resolving `catalog:<name>` specifiers
    /// via `resolve_catalog(catalog_name, target_pkg_name)`.
    #[must_use]
    pub fn from_scoped_inputs_with_catalog<'a>(
        overrides: impl IntoIterator<Item = (&'a str, &'a str)>,
        extensions: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
        has_pnpmfile_hook: bool,
        lockfile_dir: Option<&Path>,
        resolve_catalog: impl Fn(&str, &str) -> Option<&'a str>,
    ) -> Self {
        Self::from_scoped_inputs_with_roots(
            overrides,
            extensions,
            has_pnpmfile_hook,
            (lockfile_dir, lockfile_dir),
            resolve_catalog,
        )
    }

    /// Like [`Self::from_scoped_inputs_with_catalog`], rebasing relative
    /// override targets from `overrides_root_dir` onto `lockfile_dir` when they
    /// differ.
    #[must_use]
    pub fn from_scoped_inputs_with_roots<'a>(
        overrides: impl IntoIterator<Item = (&'a str, &'a str)>,
        extensions: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
        _has_pnpmfile_hook: bool,
        (overrides_root_dir, lockfile_dir): (Option<&Path>, Option<&Path>),
        resolve_catalog: impl Fn(&str, &str) -> Option<&'a str>,
    ) -> Self {
        let mut edges = Vec::new();
        for (selector, spec) in overrides {
            if let Some(edge) = parse_override_edge(
                selector,
                spec,
                (overrides_root_dir.or(lockfile_dir), lockfile_dir),
                &resolve_catalog,
            ) {
                edges.push(edge);
            }
        }
        for (parent_selector, child_alias, spec) in extensions {
            let resolved = deref_catalog(spec, child_alias, &resolve_catalog);
            if let Some(edge) =
                parse_extension_edge(parent_selector, child_alias, resolved, lockfile_dir)
            {
                edges.push(edge);
            }
        }
        Self {
            authorized_edges: (!edges.is_empty()).then(|| edges.into()),
            pristine_declared_specs: None,
            catalogs: None,
            has_after_all_resolved_hook: false,
            allow_deploy_file_url_keys: false,
            scope: WorkspaceContainmentScope {
                lockfile_dir: lockfile_dir.map(Arc::from),
                overrides_root_dir: overrides_root_dir.map(Arc::from),
                ..WorkspaceContainmentScope::default()
            },
        }
    }

    /// Derive the policy from un-scoped override and extension specifiers
    /// (convenience constructor for tests without parent selectors).
    #[must_use]
    pub fn from_hook_inputs<'spec>(
        override_specs: impl IntoIterator<Item = &'spec str>,
        extension_specs: impl IntoIterator<Item = &'spec str>,
        has_pnpmfile_hook: bool,
    ) -> Self {
        let overrides = override_specs
            .into_iter()
            .map(|spec| ("*", spec));
        let extensions = extension_specs
            .into_iter()
            .map(|spec| ("*", "*", spec));
        Self::from_scoped_inputs(overrides, extensions, has_pnpmfile_hook, None)
    }

    /// Attach workspace catalog entries so `catalog:` declarations in pristine
    /// manifests and importer specifiers resolve before range/locality checks.
    #[must_use]
    pub fn with_catalogs(mut self, catalogs: PolicyCatalogMap) -> Self {
        if !catalogs.is_empty() {
            self.catalogs = Some(Arc::new(catalogs));
        }
        self
    }

    /// Attach the active workspace containment scope relative to `lockfile_dir`.
    #[must_use]
    pub const fn with_workspace_scope(
        mut self,
        allow_deep_workspace_links: bool,
        workspace_depth_above_lockfile: Option<usize>,
    ) -> Self {
        self.scope.allow_deep_workspace_links = allow_deep_workspace_links;
        self.scope.workspace_depth_above_lockfile = workspace_depth_above_lockfile;
        self
    }

    /// Record the workspace or standalone containment root directory.
    #[must_use]
    pub fn with_workspace_dir(mut self, workspace_dir: &Path) -> Self {
        if let Some(lf_dir) = self.scope.lockfile_dir.as_deref() {
            self.scope.workspace_depth_above_lockfile =
                lf_dir.to_str().and_then(|lf| depth_under_workspace(workspace_dir, lf));
        }
        self.scope.workspace_dir = Some(Arc::from(workspace_dir));
        self
    }

    /// Record live workspace project and publish directories relative to
    /// `lockfile_dir`.
    #[must_use]
    pub fn with_live_workspace_dirs(mut self, dirs: impl IntoIterator<Item = String>) -> Self {
        let collected: Vec<String> = dirs.into_iter().collect();
        self.scope.live_workspace_dirs = Some(collected.into());
        self
    }

    /// Record the importer IDs actually installed in this run so absent or
    /// filtered-out workspace projects do not fail or seed authorizations.
    #[must_use]
    pub fn with_selected_importer_ids(mut self, ids: impl IntoIterator<Item = String>) -> Self {
        let collected: Vec<String> = ids.into_iter().collect();
        self.scope.selected_importer_ids = Some(collected.into());
        self
    }

    /// Mark whether an `afterAllResolved` hook is active.
    #[must_use]
    pub const fn with_after_all_resolved_hook(mut self, active: bool) -> Self {
        self.has_after_all_resolved_hook = active;
        self
    }

    /// Attach pristine declared specs recovered from candidate manifests.
    #[must_use]
    pub fn with_pristine_declared_specs(mut self, specs: PristineDeclaredSpecs) -> Self {
        if !specs.is_empty() {
            self.pristine_declared_specs = Some(Arc::new(specs));
        }
        self
    }

    /// Append `(parent_pkg_key, parent_name, parent_version, child_alias, rewritten_spec)`
    /// edges observed by probing the active `readPackage` hook for a specific
    /// non-local package source.
    #[must_use]
    pub fn with_probed_source_hook_rewrites(
        mut self,
        rewrites: impl IntoIterator<Item = (PkgNameVerPeer, String, Option<String>, String, String)>,
    ) -> Self {
        let mut edges: Vec<AuthorizedLocalEdge> = self.authorized_edges
            .as_deref()
            .map(<[AuthorizedLocalEdge]>::to_vec)
            .unwrap_or_default();
        let lockfile_dir = self.scope.lockfile_dir.as_deref();
        for (pkg_key, parent_name, parent_range, child_alias, spec) in rewrites {
            if let Some(targets) = build_spec_targets(&spec, lockfile_dir, lockfile_dir) {
                edges.push(AuthorizedLocalEdge {
                    parent_pkg_key: Some(pkg_key.without_peer()),
                    parent_name: Some(parent_name),
                    parent_range,
                    child_alias,
                    child_range: None,
                    targets,
                });
            }
        }
        self.authorized_edges = (!edges.is_empty()).then(|| edges.into());
        self
    }

    /// Mark whether an active `pnpm deploy` operation authorizes `file:///` keys.
    #[must_use]
    pub const fn with_deploy(mut self, allow_deploy_file_url_keys: bool) -> Self {
        self.allow_deploy_file_url_keys = allow_deploy_file_url_keys;
        self
    }

    /// Override the directory against which relative resolution targets in the
    /// lockfile are resolved.
    #[must_use]
    pub fn with_lockfile_dir(mut self, lockfile_dir: &Path) -> Self {
        if let Some(ws_dir) = self.scope.workspace_dir.as_deref() {
            self.scope.workspace_depth_above_lockfile =
                lockfile_dir.to_str().and_then(|lf| depth_under_workspace(ws_dir, lf));
        }
        self.scope.lockfile_dir = Some(Arc::from(lockfile_dir));
        self
    }

    /// Render `target` as a POSIX relative path from `self.scope.lockfile_dir`.
    #[must_use]
    pub fn relative_path_from_lockfile_dir(&self, target: &Path) -> Option<String> {
        let base = self.scope.lockfile_dir.as_deref()?;
        relative_posix_path(base, target.to_str()?)
    }

    /// Whether any configured edge or `afterAllResolved` hook can grant a
    /// policy authorization.
    #[must_use]
    pub const fn has_authorization_source(&self) -> bool {
        self.authorized_edges.is_some() || self.has_after_all_resolved_hook
    }

    /// Whether any configured override edge restricts its target by a child
    /// version range (`parent>child@^1.0.0`).
    #[must_use]
    pub fn has_child_ranged_edges(&self) -> bool {
        self.authorized_edges
            .as_deref()
            .is_some_and(|edges| {
                edges
                    .iter()
                    .any(|edge| {
                        edge.child_range
                            .as_deref()
                            .filter(|range| !range.is_empty())
                            .is_some_and(|range| range != "*")
                    })
            })
    }

    pub(super) fn pristine_declared_spec(
        &self,
        parent_base_key: &PkgNameVerPeer,
        field: LocalDepField,
        alias: String,
    ) -> Option<&str> {
        let map = self.pristine_declared_specs.as_deref()?;
        let key = (parent_base_key.clone(), field, alias);
        map.get(&key).map(String::as_str)
    }

    pub(super) fn resolve_catalog_spec<'a>(&'a self, alias: &str, spec: &'a str) -> &'a str {
        let Some(rest) = spec.strip_prefix("catalog:") else {
            return spec;
        };
        let catalog_name = if rest.is_empty() { "default" } else { rest };
        self.catalogs
            .as_deref()
            .and_then(|cats| cats.get(catalog_name))
            .and_then(|entries| entries.get(alias))
            .map_or(spec, String::as_str)
    }
}

fn depth_under_workspace(ws_dir: &Path, lockfile_str: &str) -> Option<usize> {
    let rel = relative_posix_path(ws_dir, lockfile_str)?;
    if rel == "." || rel.is_empty() {
        return Some(0);
    }
    if rel == ".." || rel.starts_with("../") {
        return None;
    }
    Some(
        rel.split('/')
            .filter(|seg| !seg.is_empty())
            .filter(|seg| *seg != ".")
            .count(),
    )
}
