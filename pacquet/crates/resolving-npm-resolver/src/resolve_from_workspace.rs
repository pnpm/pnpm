//! Port of pnpm's
//! [`tryResolveFromWorkspace`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L806-L844)
//! and its inner
//! [`tryResolveFromWorkspacePackages`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L846-L888)
//! / [`pickMatchingLocalVersionOrNull`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L890-L906)
//! / [`resolveFromLocalPackage`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L908-L951)
//! helpers.
//!
//! The npm-resolver intercepts every `workspace:`-shaped wanted
//! dependency *except* the path-relative forms (`workspace:./foo`,
//! `workspace:../bar`) — those flow through unchanged so the
//! local-resolver in the chain takes them as `link:`-shaped
//! directory specs.

use std::path::{Path, PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
use pacquet_resolving_resolver_base::{
    PkgResolutionId, ResolveResult, WantedDependency, WorkspacePackage, WorkspacePackages,
    WorkspacePackagesByVersion,
};
use pacquet_workspace_range_resolver::resolve_workspace_range;

use crate::{
    parse_bare_specifier::parse_bare_specifier,
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
    workspace_pref_to_npm::{InvalidWorkspaceSpecError, workspace_pref_to_npm},
};

/// Options threaded into [`try_resolve_from_workspace`]. Mirrors
/// upstream's per-call bag at
/// [`tryResolveFromWorkspace`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L808-L819).
#[derive(Debug, Clone)]
pub struct ResolveFromWorkspaceOptions<'a> {
    /// Workspace-relative `<root>/pnpm-workspace.yaml` directory the
    /// `link:` entry's relative path is rendered against.
    pub project_dir: &'a Path,
    /// Lockfile root. Mirrors upstream's `lockfileDir` argument —
    /// `link:`-shaped resolutions render relative to `project_dir`
    /// regardless, but `file:`-shaped (injected) resolutions use this
    /// as the relativity anchor.
    pub lockfile_dir: &'a Path,
    /// Registry URL passed through to [`parse_bare_specifier`] so
    /// `npm:<alias>@<version>` outputs flow through the same parsing
    /// path as a plain npm spec.
    pub registry: &'a str,
    /// Default tag (`latest`) the parser falls back on when the
    /// translated spec is bare (no version after the alias).
    pub default_tag: &'a str,
    /// Workspace packages map indexed by name → version → manifest.
    /// `None` when the install caller never populated one; this
    /// surfaces as a `WORKSPACE_PACKAGES_NOT_LOADED` error.
    pub workspace_packages: Option<&'a WorkspacePackages>,
    /// `true` materialises the dependency as a `file:` (hard-linked
    /// copy) resolution instead of a `link:` symlink. Mirrors upstream's
    /// `injectWorkspacePackages` + per-dep `injected` toggle.
    pub inject_workspace_packages: bool,
}

/// Error envelope for [`try_resolve_from_workspace`]. The two pnpm
/// codes (`WORKSPACE_PKG_NOT_FOUND`, `NO_MATCHING_VERSION_INSIDE_WORKSPACE`)
/// are reproduced verbatim.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveFromWorkspaceError {
    /// The translated workspace spec failed to parse. Mirrors
    /// upstream's `throw new Error('Invalid workspace: spec ...')`
    /// branch — practically unreachable since the caller checks the
    /// `workspace:` prefix before invoking this entry point.
    #[diagnostic(transparent)]
    InvalidWorkspaceSpec(#[error(source)] InvalidWorkspaceSpecError),

    /// `workspace_packages` was `None`. Mirrors upstream's
    /// [`Cannot resolve package from workspace because opts.workspacePackages is not defined`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L829)
    /// throw.
    #[display(
        "Cannot resolve package from workspace because workspace packages were not loaded into the resolver"
    )]
    #[diagnostic(code(pacquet_resolving_npm_resolver::workspace_packages_not_loaded))]
    WorkspacePackagesNotLoaded,

    /// The npm parser refused the translated bare specifier. Mirrors
    /// upstream's `throw new Error('Invalid workspace: spec (${...})')`.
    #[display("Invalid workspace: spec ({bare_specifier})")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::invalid_workspace_translated_spec))]
    UnparsableSpec {
        #[error(not(source))]
        bare_specifier: String,
    },

    /// Workspace map didn't carry the requested package name. Mirrors
    /// pnpm's
    /// [`WORKSPACE_PKG_NOT_FOUND`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L862-L868).
    #[display(
        "In {project_dir}: \"{name}@{bare_specifier}\" is in the dependencies but no package named \"{name}\" is present in the workspace"
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PKG_NOT_FOUND), help("{hint}"))]
    WorkspacePkgNotFound {
        name: String,
        bare_specifier: String,
        project_dir: String,
        #[error(not(source))]
        hint: String,
    },

    /// Workspace map carried the name but no version satisfied the
    /// range. Mirrors pnpm's
    /// [`NO_MATCHING_VERSION_INSIDE_WORKSPACE`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L877-L885).
    #[display(
        "In {project_dir}: No matching version found for {alias}@{bare_specifier} inside the workspace{available}"
    )]
    #[diagnostic(code(ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE))]
    NoMatchingVersionInsideWorkspace {
        alias: String,
        bare_specifier: String,
        project_dir: String,
        #[error(not(source))]
        available: String,
    },
}

/// Try to resolve a `workspace:` wanted dependency against the project
/// workspace. Returns `Ok(None)` when the wanted dep isn't
/// workspace-prefixed (so the caller can fall through to the npm
/// path); returns `Ok(Some(_))` with a `link:` / `file:` resolution
/// otherwise.
///
/// Mirrors upstream's
/// [`tryResolveFromWorkspace`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L806-L844).
pub fn try_resolve_from_workspace(
    wanted_dependency: &WantedDependency,
    opts: &ResolveFromWorkspaceOptions<'_>,
) -> Result<Option<ResolveResult>, ResolveFromWorkspaceError> {
    let Some(bare) = wanted_dependency.bare_specifier.as_deref() else {
        return Ok(None);
    };
    if !bare.starts_with("workspace:") {
        return Ok(None);
    }
    // `workspace:./foo` / `workspace:../foo` are owned by the local
    // resolver; let those fall through so the chain doesn't claim
    // them here.
    if bare.starts_with("workspace:.") {
        return Ok(None);
    }

    let translated =
        workspace_pref_to_npm(bare).map_err(ResolveFromWorkspaceError::InvalidWorkspaceSpec)?;
    let spec = parse_bare_specifier(
        &translated,
        wanted_dependency.alias.as_deref(),
        opts.default_tag,
        opts.registry,
    )
    .ok_or_else(|| ResolveFromWorkspaceError::UnparsableSpec {
        bare_specifier: bare.to_string(),
    })?;

    let workspace_packages =
        opts.workspace_packages.ok_or(ResolveFromWorkspaceError::WorkspacePackagesNotLoaded)?;

    let result =
        try_resolve_from_workspace_packages(workspace_packages, &spec, wanted_dependency, opts)?;
    Ok(Some(result))
}

pub(crate) fn try_resolve_from_workspace_packages(
    workspace_packages: &WorkspacePackages,
    spec: &RegistryPackageSpec,
    wanted_dependency: &WantedDependency,
    opts: &ResolveFromWorkspaceOptions<'_>,
) -> Result<ResolveResult, ResolveFromWorkspaceError> {
    let matching_name = workspace_packages.get(spec.name.as_str()).ok_or_else(|| {
        let names = workspace_packages.keys().cloned().collect::<Vec<_>>().join(", ");
        ResolveFromWorkspaceError::WorkspacePkgNotFound {
            name: spec.name.clone(),
            bare_specifier: wanted_dependency.bare_specifier.clone().unwrap_or_default(),
            project_dir: opts.project_dir.display().to_string(),
            hint: format!("Packages found in the workspace: {names}"),
        }
    })?;

    let picked = pick_matching_local_version_or_null(matching_name, spec).ok_or_else(|| {
        let mut versions: Vec<String> = matching_name.keys().cloned().collect();
        versions.sort_by(|a, b| rcompare_versions(a, b));
        let available = if versions.is_empty() {
            String::new()
        } else {
            format!(". Available versions: {}", versions.join(", "))
        };
        ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace {
            alias: wanted_dependency.alias.clone().unwrap_or_default(),
            bare_specifier: wanted_dependency.bare_specifier.clone().unwrap_or_default(),
            project_dir: opts.project_dir.display().to_string(),
            available,
        }
    })?;
    let local_package =
        matching_name.get(&picked).expect("picked version came from the matching set");

    Ok(resolve_from_local_package(
        local_package,
        wanted_dependency,
        opts.inject_workspace_packages || wanted_dependency.injected.unwrap_or(false),
        opts.project_dir,
        opts.lockfile_dir,
    ))
}

/// Mirror upstream's
/// [`pickMatchingLocalVersionOrNull`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L890-L906).
pub(crate) fn pick_matching_local_version_or_null(
    versions: &WorkspacePackagesByVersion,
    spec: &RegistryPackageSpec,
) -> Option<String> {
    match spec.spec_type {
        RegistryPackageSpecType::Tag => {
            let raw: Vec<String> = versions.keys().cloned().collect();
            resolve_workspace_range("*", &raw)
        }
        RegistryPackageSpecType::Version => {
            versions.contains_key(&spec.fetch_spec).then(|| spec.fetch_spec.clone())
        }
        RegistryPackageSpecType::Range => {
            let raw: Vec<String> = versions.keys().cloned().collect();
            resolve_workspace_range(&spec.fetch_spec, &raw)
        }
    }
}

/// Mirror upstream's
/// [`resolveFromLocalPackage`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L908-L951).
///
/// The TS branch also derives a `normalizedBareSpecifier` for the
/// add / update paths via `calcSpecifierForWorkspaceDep`; pacquet
/// doesn't carry the pinned-version / save-workspace-protocol config
/// through to the resolver yet, so the field stays `None` until those
/// land.
pub(crate) fn resolve_from_local_package(
    local_package: &WorkspacePackage,
    wanted_dependency: &WantedDependency,
    hard_link_local_packages: bool,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> ResolveResult {
    let local_dir = resolve_local_package_dir(local_package);

    let (id_text, directory) = if hard_link_local_packages {
        let relative_to_lockfile = forward_slashes(relative_path(lockfile_dir, &local_dir));
        let id = format!("file:{relative_to_lockfile}");
        (id, relative_to_lockfile)
    } else {
        let relative_to_project = forward_slashes(relative_path(project_dir, &local_dir));
        let id = format!("link:{relative_to_project}");
        (id, relative_to_project)
    };

    ResolveResult {
        id: PkgResolutionId::from(id_text),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(local_package.manifest.clone())),
        resolution: LockfileResolution::Directory(DirectoryResolution { directory }),
        resolved_via: "workspace".to_string(),
        normalized_bare_specifier: None,
        alias: wanted_dependency.alias.clone(),
        policy_violation: None,
    }
}

/// Mirror upstream's
/// [`resolveLocalPackageDir`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L992-L998).
/// Honours `publishConfig.directory` when `publishConfig.linkDirectory`
/// is unset or `true`; otherwise the project's own `rootDir`.
fn resolve_local_package_dir(local_package: &WorkspacePackage) -> PathBuf {
    let publish_config = local_package.manifest.get("publishConfig");
    let publish_dir =
        publish_config.and_then(|cfg| cfg.get("directory")).and_then(serde_json::Value::as_str);
    let link_directory = publish_config
        .and_then(|cfg| cfg.get("linkDirectory"))
        .and_then(serde_json::Value::as_bool);
    if publish_dir.is_none() || link_directory == Some(false) {
        return local_package.root_dir.clone();
    }
    local_package.root_dir.join(publish_dir.expect("guard above"))
}

fn relative_path(base: &Path, target: &Path) -> String {
    pathdiff_string(base, target).unwrap_or_else(|| target.display().to_string())
}

fn forward_slashes(input: String) -> String {
    if input.contains('\\') { input.replace('\\', "/") } else { input }
}

/// Tiny pathdiff fallback. The npm-resolver crate doesn't pull in
/// `pathdiff` today; this helper keeps the dependency footprint
/// unchanged.
fn pathdiff_string(base: &Path, target: &Path) -> Option<String> {
    use std::path::Component;

    let mut base_components: Vec<Component<'_>> = base.components().collect();
    let mut target_components: Vec<Component<'_>> = target.components().collect();

    let mut common = 0;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }
    base_components.drain(..common);
    target_components.drain(..common);

    let mut out = PathBuf::new();
    for _ in base_components.iter().filter(|component| !matches!(component, Component::CurDir)) {
        out.push("..");
    }
    for component in target_components {
        out.push(component.as_os_str());
    }
    // `base == target` (a workspace package depending on itself) yields an
    // empty relative path, which must stay empty: pnpm renders the id as
    // `link:` (bare), matching `path.relative(projectDir, projectDir) === ''`
    // — not `link:.`.
    Some(out.display().to_string())
}

/// Compare semver versions in *descending* order for the "available
/// versions" hint. Versions that don't parse fall back to lexicographic
/// reverse so the message at least stays stable.
fn rcompare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left_parsed), Ok(right_parsed)) => right_parsed.cmp(&left_parsed),
        _ => right.cmp(left),
    }
}

#[cfg(test)]
mod tests;
