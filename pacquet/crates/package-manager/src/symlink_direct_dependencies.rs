use crate::{
    SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, link_direct_dep_bins,
    symlink_package,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::LinkBinsError;
use pacquet_config::Config;
use pacquet_lockfile::{
    ImporterDepVersion, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencySpec,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::{
    AddedRoot, DependencyType, LogEvent, LogLevel, Reporter, RootLog, RootMessage,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// Create the `node_modules/` symlinks for every importer in the lockfile.
///
/// For each `importers.<id>` entry:
///
/// - Resolve the importer's `rootDir = workspace_root.join(id)` (with
///   `id == "."` meaning the workspace root itself).
/// - For every direct dependency in the importer's groups, create the
///   appropriate symlink under `rootDir/node_modules/`. Snapshots that
///   resolve through the shared virtual store get a link to
///   `<virtual_store_dir>/<name>@<ver>/node_modules/<name>`. `link:`
///   snapshots (cross-importer `workspace:` deps) get a direct symlink
///   to the dependee's `rootDir`.
/// - Emit one `pnpm:root added` per direct dependency with the
///   importer's `rootDir` as the event prefix, matching upstream's
///   per-project emit at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L131>.
///
/// The virtual store dir (`config.virtual_store_dir`) stays singular
/// across the install — only the per-project `node_modules/` and its
/// symlinks fan out. By default `pacquet_config::default_virtual_store_dir`
/// anchors it at `<workspace_root>/node_modules/.pnpm` (matching pnpm),
/// but the actual location is whatever the resolved `Config` field
/// holds — `pnpm-workspace.yaml`'s `virtualStoreDir` can move it.
#[must_use]
pub struct SymlinkDirectDependencies<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub config: &'static Config,
    /// Install-scoped slot-directory mapping (GVS-aware). Drives the
    /// per-direct-dep symlink target — `node_modules/<dep>` resolves
    /// to `layout.slot_dir(<key>)/node_modules/<dep>`. See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a VirtualStoreLayout,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: DependencyGroupList,
    /// Workspace root. For a single-project install this is the
    /// directory containing the user's `package.json`; for a real
    /// workspace it's the directory containing `pnpm-workspace.yaml`.
    /// Same value as the `lockfileDir` upstream pnpm uses for
    /// `pnpm:stage` / `pnpm:summary` events.
    pub workspace_root: &'a Path,
    /// Snapshots the installability pass marked optional+incompatible.
    /// A direct dep whose resolved snapshot key is in this set is
    /// omitted from `node_modules/<name>` (no symlink, no
    /// `pnpm:root added` event, no bin linking). Mirrors pnpm's
    /// `linkDirectDeps` walk skipping entries whose `depPath` is
    /// in `skipPkgIds`.
    pub skipped: &'a SkippedSnapshots,

    /// When `true`, skip every direct dep whose resolved version
    /// is [`ImporterDepVersion::Regular`] and only materialize
    /// [`ImporterDepVersion::Link`] entries — workspace siblings
    /// resolved through `workspace:*` / `link:`. Used by the
    /// hoisted linker to layer workspace-sibling symlinks on top
    /// of the real-directory tree the slice 5 linker produced;
    /// the regular deps already landed under
    /// `<importer>/node_modules/<alias>/` as real directories
    /// from the hoisted linker, and re-symlinking them would
    /// either no-op or corrupt the layout.
    ///
    /// Mirrors upstream's hoisted branch at
    /// [`installing/deps-restorer/src/index.ts:411-440`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L411-L440)
    /// where `symlinkDirectDependencies` runs after
    /// `linkHoistedModules` with a filtered `directDependenciesByImporterId`
    /// containing only `link:`-shaped entries.
    pub link_only: bool,

    /// `<alias → resolved-target-path>` for every transitive that the
    /// hoist pass will publicly hoist into the root's `node_modules/`.
    /// Folded into the dedupe map alongside the root importer's direct
    /// deps so a non-root importer's direct dep resolving to the same
    /// target as a publicly-hoisted alias is also deduped — matching
    /// pnpm where `linkDirectDepsAndDedupe` reads root's `node_modules/`
    /// *after* the hoist pass already populated it. Pacquet's pipeline
    /// runs hoist after this step, so the caller pre-computes the
    /// hoist plan ([`crate::get_hoisted_dependencies`]) and threads
    /// the public-side targets in here.
    pub public_hoist_targets: Option<&'a BTreeMap<String, PathBuf>>,
}

/// Error type of [`SymlinkDirectDependencies`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum SymlinkDirectDependenciesError {
    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    /// A lockfile importer key that would escape the workspace root.
    /// Pnpm's lockfile spec uses POSIX relative paths for importer
    /// keys (e.g. `packages/web`); a key that is absolute, contains
    /// `..` traversal, or carries a Windows drive prefix is treated
    /// as a malformed lockfile so we don't end up creating
    /// `node_modules` outside the workspace. Upstream pnpm does not
    /// guard this explicitly, but the importer keys it writes are
    /// always relative POSIX paths under the workspace root — so
    /// this check is parity-preserving on conforming input.
    #[display("Refusing to install importer with unsafe path key {importer_id:?}")]
    #[diagnostic(
        code(pacquet_package_manager::unsafe_importer_path),
        help(
            "Importer keys in pnpm-lock.yaml must be POSIX paths relative to the workspace root (e.g. `packages/web`). Absolute paths, drive prefixes, and `..` components are rejected."
        )
    )]
    UnsafeImporterPath {
        #[error(not(source))]
        importer_id: String,
    },

    /// Surfaces a per-package symlink failure (e.g. permission denied,
    /// disk full, an existing non-symlink file). Replaces the prior
    /// `expect("symlink pkg")` which panicked inside a rayon task and
    /// took the whole install down.
    #[display("Failed to symlink {name:?} for importer {importer_id:?}: {source}")]
    #[diagnostic(code(pacquet_package_manager::symlink_failed))]
    SymlinkPackage {
        importer_id: String,
        name: String,
        #[error(source)]
        source: SymlinkPackageError,
    },
}

impl<DependencyGroupList> SymlinkDirectDependencies<'_, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Execute the subroutine.
    pub fn run<Reporter: self::Reporter>(self) -> Result<(), SymlinkDirectDependenciesError> {
        let SymlinkDirectDependencies {
            config,
            layout,
            importers,
            dependency_groups,
            workspace_root,
            skipped,
            link_only,
            public_hoist_targets,
        } = self;

        // Collect once so the same group order can drive every importer.
        // Upstream calls `linkDirectDeps` once with a per-importer
        // `dependencies` list, so the group order is shared across all
        // importers anyway.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();

        // Each importer's modules dir is `<importer_root>/<modules_dir_basename>`.
        // Pnpm's `modulesDir` setting is a directory name (a single
        // component, default `node_modules`) applied uniformly under
        // every importer. Pacquet stores `config.modules_dir` as a
        // full path anchored at the workspace root, so peel off the
        // last component to get the per-importer suffix — that way a
        // `modulesDir: custom_modules` override in
        // `pnpm-workspace.yaml` propagates to every importer instead
        // of leaving the symlink stage stuck on `node_modules` while
        // other stages (`.modules.yaml` writing, bin linking) use
        // `config.modules_dir`.
        let modules_dir_name: &OsStr =
            config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));

        // Sorted iteration so `pnpm:root` event order stays
        // deterministic. The wire shape doesn't require this, but a
        // deterministic order makes assertions in tests (and the
        // upstream snapshot tests we will be porting) tractable.
        let mut keys: Vec<&str> = importers.keys().map(String::as_str).collect();
        keys.sort_unstable();

        // `dedupeDirectDeps` short-circuits in pnpm when there is no
        // root importer or only one importer total — there's nothing
        // to dedupe against. Mirrors the guard at
        // [`installing/linking/direct-dep-linker/src/linkDirectDeps.ts:34`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L34).
        let dedupe = config.dedupe_direct_deps && importers.contains_key(".") && keys.len() > 1;
        let root_targets: Option<BTreeMap<String, PathBuf>> = dedupe.then(|| {
            let root_project_dir = importer_root_dir(workspace_root, ".");
            let mut targets = collect_resolved_targets(
                layout,
                &importers["."],
                &root_project_dir,
                dependency_groups.iter().copied(),
                skipped,
                link_only,
            );
            // Fold publicly-hoisted aliases in alongside root's
            // direct deps. Pnpm's `linkDirectDepsAndDedupe` reads
            // root's `node_modules/` after the hoist pass populates
            // it, so its dedupe naturally covers both kinds; pacquet
            // runs hoist *after* this step, so the caller pre-computes
            // the hoist plan and feeds the public-side targets here.
            // Direct deps win on collision — a root direct dep won't
            // be silently overwritten by a hoist plan entry that
            // resolves to a different slot.
            if let Some(extra) = public_hoist_targets {
                for (alias, target) in extra {
                    targets.entry(alias.clone()).or_insert_with(|| target.clone());
                }
            }
            targets
        });

        for importer_id in keys {
            // Reject importer keys that would escape the workspace
            // root. A malformed (or hostile) lockfile could otherwise
            // make `Path::join` create `node_modules` outside the
            // workspace — `Path::join` discards the base when the
            // RHS is absolute, and `..` components are otherwise
            // permitted.
            validate_importer_id(importer_id)?;
            // Safe: we just iterated `importers.keys()`.
            let project_snapshot = &importers[importer_id];
            let project_dir = importer_root_dir(workspace_root, importer_id);
            let modules_dir = project_dir.join(modules_dir_name);

            // Only non-root importers get deduped against root.
            // Mirrors pnpm's `linkDirectDepsAndDedupe`, which links
            // the root project unfiltered and then trims each
            // sibling's list against what root just linked.
            let dedupe_against = match (&root_targets, importer_id) {
                (Some(targets), id) if id != "." => Some(targets),
                _ => None,
            };

            link_one_importer::<Reporter>(
                importer_id,
                layout,
                project_snapshot,
                &project_dir,
                &modules_dir,
                dependency_groups.iter().copied(),
                skipped,
                link_only,
                dedupe_against,
            )?;
        }

        Ok(())
    }
}

/// Reject importer keys that would resolve outside the workspace root.
///
/// Pnpm's lockfile spec writes importer keys as POSIX paths relative
/// to the workspace root (`.` for the root, `packages/web` for a
/// subproject). Anything else — an absolute POSIX path, a Windows
/// drive prefix, a `..` segment — is either malformed or hostile, so
/// surface it as a typed error rather than silently letting
/// `Path::join` produce an off-workspace path.
fn validate_importer_id(importer_id: &str) -> Result<(), SymlinkDirectDependenciesError> {
    let unsafe_path = || SymlinkDirectDependenciesError::UnsafeImporterPath {
        importer_id: importer_id.to_string(),
    };

    // `.` is the canonical root importer key. An empty string is
    // non-standard — pnpm never writes one — and conflating it with
    // `.` would mask malformed lockfiles, so reject it explicitly.
    if importer_id == "." {
        return Ok(());
    }
    if importer_id.is_empty() {
        return Err(unsafe_path());
    }

    // Absolute POSIX path. Pnpm writes relative paths; an absolute
    // value would cause `Path::join` to discard `workspace_root`.
    if importer_id.starts_with('/') {
        return Err(unsafe_path());
    }
    // Windows drive prefix (e.g. `C:` or `C:/foo`). Same blast radius
    // as the absolute POSIX case on Windows hosts.
    let bytes = importer_id.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(unsafe_path());
    }
    // Backslash separator. Pnpm writes POSIX `/`; a backslash key
    // would be either a Windows-native path or pnpm-incompatible
    // garbage.
    if importer_id.contains('\\') {
        return Err(unsafe_path());
    }
    // Any `..` segment. Mirrors `path::Component::ParentDir` rejection
    // without paying for full component iteration since importer keys
    // are tiny.
    for segment in importer_id.split('/') {
        if segment == ".." {
            return Err(unsafe_path());
        }
    }

    Ok(())
}

/// Collect the direct-dependency *names* for `snapshot`, applying
/// the same first-wins / skipped / link-vs-regular filters that
/// `link_one_importer` (private to this module) uses to drive the
/// symlink + bin-link pass. Public so the post-`BuildModules`
/// top-level bin pass in [`crate::InstallFrozenLockfile::run`]
/// can run with the same per-importer name set the symlink phase
/// saw, without re-implementing the filter logic in two places.
///
/// `link_only` mirrors the [`SymlinkDirectDependencies::link_only`]
/// flag — when `true`, only `link:` workspace siblings survive the
/// filter (used by the hoisted-linker re-link pass; the regular
/// deps live as real directories under
/// `<importer>/node_modules/<alias>` already and don't need the
/// symlink-targeted filter).
pub fn direct_dep_names_for_importer<Iter>(
    snapshot: &ProjectSnapshot,
    dependency_groups: Iter,
    skipped: &SkippedSnapshots,
    link_only: bool,
) -> Vec<String>
where
    Iter: IntoIterator<Item = DependencyGroup>,
{
    let mut seen: HashSet<&PkgName> = HashSet::new();
    dependency_groups
        .into_iter()
        .filter(|group| !matches!(group, DependencyGroup::Peer))
        .flat_map(|group| snapshot.get_map_by_group(group).into_iter().flatten())
        .filter(|(name, _)| seen.insert(*name))
        .filter(|(name, spec)| match spec.version.resolved_key(name) {
            Some(resolved) => !skipped.contains(&resolved),
            // `link:` deps have no virtual-store slot and so cannot be
            // in `skipped` — keep them.
            None => true,
        })
        .filter(
            |(_, spec)| {
                if link_only { matches!(spec.version, ImporterDepVersion::Link(_)) } else { true }
            },
        )
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Resolve `importer_id` (a lockfile key) against the workspace root.
///
/// Pnpm's lockfile spec uses `"."` for the root importer and
/// forward-slash POSIX paths for sub-importers. Mirroring that here
/// keeps lockfiles written by pacquet and pnpm interchangeable. The
/// returned path is platform-native (`Path::join` handles the
/// conversion on Windows).
pub(crate) fn importer_root_dir(workspace_root: &Path, importer_id: &str) -> PathBuf {
    if importer_id == "." {
        workspace_root.to_path_buf()
    } else {
        // `importer_id` is POSIX in the lockfile; `Path::join` accepts
        // forward slashes and converts to native separators. The
        // empty-key case is rejected upstream by
        // [`validate_importer_id`], so this branch only runs on
        // POSIX-relative sub-importer paths.
        workspace_root.join(importer_id)
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameters are independent inputs; bundling them into a struct would not improve clarity"
)]
fn link_one_importer<Reporter: self::Reporter>(
    importer_id: &str,
    layout: &VirtualStoreLayout,
    project_snapshot: &ProjectSnapshot,
    project_dir: &Path,
    modules_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
    dedupe_against: Option<&BTreeMap<String, PathBuf>>,
) -> Result<(), SymlinkDirectDependenciesError> {
    let entries = collect_resolved_entries(
        layout,
        project_snapshot,
        project_dir,
        dependency_groups,
        skipped,
        link_only,
    );

    // `dedupeDirectDeps`: drop any entry whose resolved target dir
    // matches what the root importer resolved the same alias to.
    // Mirrors `omitDepsFromRoot` at
    // [`installing/linking/direct-dep-linker/src/linkDirectDeps.ts:66-72`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L66-L72):
    // pnpm's `pathsEqual` is `path.relative(a, b) === ''`, which on
    // already-absolute paths reduces to lexical equality. Pacquet's
    // target paths are always absolute (slot dirs come from the
    // layout; link targets join against an absolute `project_dir`),
    // so `PathBuf` equality matches that semantics without paying a
    // canonicalize. Bins follow: if a deduped alias is not in
    // `entries`, `link_direct_dep_bins` won't see it either.
    let entries: Vec<ResolvedEntry<'_>> = if let Some(root_targets) = dedupe_against {
        entries
            .into_iter()
            .filter(|entry| {
                root_targets
                    .get(&entry.name_str)
                    .is_none_or(|root_target| root_target != &entry.target)
            })
            .collect()
    } else {
        entries
    };

    // `prefix` for the `pnpm:root` envelope. Upstream uses the
    // project's `rootDir` so the JS reporter can scope progress to
    // the right project — `lockfileDir` is reserved for the install-
    // wide stage / summary events. See
    // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L131>.
    let prefix = project_dir.to_string_lossy().into_owned();

    // `try_for_each` short-circuits on the first error and returns it
    // to the caller, replacing the prior `expect("symlink pkg")` that
    // panicked the rayon worker on any FS failure. The full result
    // collection forces every task to settle before we surface a
    // single error.
    entries.par_iter().try_for_each(|entry| -> Result<(), SymlinkDirectDependenciesError> {
        let ResolvedEntry { name, spec, group, name_str, target } = entry;

        symlink_package(target, &modules_dir.join(name_str)).map_err(|source| {
            SymlinkDirectDependenciesError::SymlinkPackage {
                importer_id: importer_id.to_string(),
                name: name_str.clone(),
                source,
            }
        })?;

        // `pnpm:root added` mirrors pnpm's emit at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L131>:
        // one event per direct dependency once the symlink has
        // been created. pacquet's frozen-lockfile snapshot doesn't
        // preserve npm-alias keys at this layer, so `realName`
        // mirrors `name`; the optional `id` / `latest` /
        // `linkedFrom` fields are out of pacquet's reach today
        // and skip from the wire shape rather than serializing as
        // JSON `null`.
        let dependency_type = match group {
            DependencyGroup::Prod => DependencyType::Prod,
            DependencyGroup::Dev => DependencyType::Dev,
            DependencyGroup::Optional => DependencyType::Optional,
            // Filtered upfront. See the comment on the `entries`
            // builder above.
            DependencyGroup::Peer => {
                unreachable!("peers are filtered out before this point")
            }
        };
        // For a `link:` dep, upstream's `version` field is the
        // resolved `link:<path>` payload (re-prepended on the
        // wire) so reporters can render the link target. Pacquet
        // mirrors that here; for `Regular` deps we keep the
        // semver-only formatting upstream uses on the wire. For
        // an `Alias`, the wire shape is the same as `Regular`
        // (the version-without-peer of the alias's resolved
        // suffix); the resolved package name surfaces via
        // `real_name` below.
        let version = match &spec.version {
            ImporterDepVersion::Regular(ver) => Some(ver.version().to_string()),
            ImporterDepVersion::Alias(alias) => Some(alias.suffix.version().to_string()),
            ImporterDepVersion::Link(target) => Some(format!("link:{target}")),
            ImporterDepVersion::File(target) => Some(format!("file:{target}")),
        };
        // For aliases, `real_name` is the resolved package's true
        // name (different from the importer-map key). For the
        // other arms the two match.
        let real_name = match &spec.version {
            ImporterDepVersion::Alias(alias) => alias.name.to_string(),
            ImporterDepVersion::Regular(_)
            | ImporterDepVersion::Link(_)
            | ImporterDepVersion::File(_) => name.to_string(),
        };
        Reporter::emit(&LogEvent::Root(RootLog {
            level: LogLevel::Debug,
            message: RootMessage::Added {
                prefix: prefix.clone(),
                added: AddedRoot {
                    name: name_str.clone(),
                    real_name,
                    version,
                    dependency_type: Some(dependency_type),
                    id: None,
                    latest: None,
                    linked_from: None,
                },
            },
        }));
        Ok(())
    })?;

    // After the symlinks exist, walk them to discover each
    // direct dep's `package.json` and link declared bins into
    // `<modules_dir>/.bin`. Mirrors pnpm v11's `linkBinsOfPackages`
    // call site for direct deps.
    let dep_names: Vec<String> = entries.iter().map(|entry| entry.name_str.clone()).collect();
    link_direct_dep_bins(modules_dir, &dep_names)
        .map_err(SymlinkDirectDependenciesError::LinkBins)?;

    Ok(())
}

/// One direct-dep entry plus its resolved on-disk target. The
/// target is computed eagerly so dedupe can compare it against the
/// root importer's targets and so the parallel symlink loop doesn't
/// recompute it.
struct ResolvedEntry<'a> {
    name: &'a PkgName,
    spec: &'a ResolvedDependencySpec,
    group: DependencyGroup,
    name_str: String,
    target: PathBuf,
}

/// Walk an importer snapshot's dependency groups and emit one
/// [`ResolvedEntry`] per direct dep, applying the same first-wins /
/// skipped / link-only filters that `link_one_importer` (private to
/// this module) uses to drive the symlink + bin-link pass.
///
/// Iterate per group so each emit can label the dependency with its
/// [`DependencyType`]. pnpm's reporter renders the diff with that
/// hint, so dropping it would silently misclassify devDependencies
/// as prod. [`ProjectSnapshot::dependencies_by_groups`] flattens the
/// groups together, which is convenient for the symlink loop but
/// loses the per-group identity we need for the emit.
///
/// Peers are filtered upfront: pnpm doesn't emit `pnpm:root` for
/// peer dependencies (they're materialised through their host
/// package, not directly under `node_modules/`), and
/// [`ProjectSnapshot::get_map_by_group`] also returns `None` for
/// `Peer` so this filter is belt-and-braces.
///
/// First-wins dedup with a `HashSet<&PkgName>`. A v9 lockfile pnpm
/// itself wrote shouldn't list the same package across multiple
/// importer sections (pnpm's resolver normalises: a package with
/// `optional: true` lands in `optionalDependencies` only). But
/// pacquet ingests user-supplied lockfiles, and a malformed one
/// with the same key in two sections would race two
/// `symlink_package` calls to the same `node_modules/<name>` and
/// emit duplicate `pnpm:root added` events. First-wins picks up
/// the highest-priority group from the caller-supplied
/// `dependency_groups` order. The CLI today passes
/// `[Prod, Dev, Optional]`, matching pnpm's
/// dependencies-over-optional precedence.
fn collect_resolved_entries<'a>(
    layout: &VirtualStoreLayout,
    project_snapshot: &'a ProjectSnapshot,
    project_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
) -> Vec<ResolvedEntry<'a>> {
    let mut seen: HashSet<&PkgName> = HashSet::new();
    dependency_groups
        .into_iter()
        .filter(|group| !matches!(group, DependencyGroup::Peer))
        .flat_map(|group| {
            project_snapshot
                .get_map_by_group(group)
                .into_iter()
                .flatten()
                .map(move |(name, spec)| (name, spec, group))
        })
        .filter(|(name, _, _)| seen.insert(*name))
        // Drop direct deps whose resolved snapshot landed in the
        // skipped set. Without this filter, the symlink would
        // either dangle (no virtual-store slot was created) or —
        // worse — point at a half-installed slot from a prior
        // install. Mirrors pnpm's `linkDirectDeps` walk skipping
        // entries whose `depPath` is in `skipPkgIds`. `link:` deps
        // never participate in the virtual store, so they are
        // exempt from the skipped check (the resolved snapshot key
        // wouldn't exist in the set anyway).
        .filter(|(name, spec, _)| match spec.version.resolved_key(name) {
            Some(resolved) => !skipped.contains(&resolved),
            // `link:` deps have no virtual-store slot and so
            // cannot be in `skipped` — keep them.
            None => true,
        })
        // Hoisted-mode filter: `link_only` keeps only `link:`
        // entries (workspace siblings) and drops every regular
        // dep. The hoisted linker (slice 5) already materialized
        // those regular deps as real `<importer>/node_modules/<alias>/`
        // directories; re-symlinking them here would either no-op
        // or replace the real dir with a slot symlink that points
        // at a slot that doesn't exist under hoisted.
        .filter(
            |(_, spec, _)| {
                if link_only { matches!(spec.version, ImporterDepVersion::Link(_)) } else { true }
            },
        )
        .map(|(name, spec, group)| {
            let name_str = name.to_string();
            let target = resolve_target_path(layout, project_dir, name, spec, &name_str);
            ResolvedEntry { name, spec, group, name_str, target }
        })
        .collect()
}

/// Map a `(name, spec)` to the on-disk path a direct-dep symlink
/// should point at. Pulled out of the rayon loop so `collect_resolved_targets`
/// can reuse the same computation when building the dedupe map.
fn resolve_target_path(
    layout: &VirtualStoreLayout,
    project_dir: &Path,
    name: &PkgName,
    spec: &ResolvedDependencySpec,
    name_str: &str,
) -> PathBuf {
    match &spec.version {
        ImporterDepVersion::Regular(ver_peer) => {
            // Route the slot-directory lookup through the
            // install-scoped [`VirtualStoreLayout`] so the path
            // works under both legacy
            // (`<virtual_store_dir>/<flat-name>`) and GVS
            // (`<global_virtual_store_dir>/<scope>/<name>/<version>/<hash>`)
            // layouts. The layout's GVS-suffix map is keyed by the
            // full snapshot key (with peer suffix), so construct
            // that from the importer's resolved version-with-peer
            // rather than from `name`+`version` separately.
            let dep_key = PkgNameVerPeer::new(PkgName::clone(name), ver_peer.clone());
            layout.slot_dir(&dep_key).join("node_modules").join(name_str)
        }
        ImporterDepVersion::Alias(alias) => {
            // For an alias, the snapshot key carries the resolved
            // package's real name + version-with-peer, and the inner
            // `node_modules/<real-name>` directory is named after that
            // real name (not the importer-map key). The on-disk
            // symlink at `<modules_dir>/<importer-key>` still uses
            // `name_str` as the link name. Mirrors pnpm's
            // `linkDirectDeps` behavior for aliased deps.
            layout.slot_dir(alias).join("node_modules").join(alias.name.to_string())
        }
        ImporterDepVersion::Link(target) => {
            // `link:<path>` values are relative to the importer's
            // `rootDir` (or absolute). Resolve them here so the
            // on-disk symlink points at the right sibling project.
            // Pnpm does the same conversion in `lockfileToDepGraph` —
            // <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts>
            // — but pacquet's lockfile snapshot already carries the
            // raw `link:` payload, so the resolution lives at the
            // install layer.
            //
            // Run the joined result through `lexical_normalize` so
            // the dedupe pass treats `<workspace>/packages/a` and
            // `<workspace>/packages/foo/../a` as the same target.
            // Pnpm's `linkDirectDepsAndDedupe` compares stored
            // symlink targets via `path.relative(a, b) === ''`,
            // which on absolute paths reduces to lexical equality
            // *after* both arguments pass through `path.resolve`
            // (Node normalises by default). `Path::join` does not,
            // so we have to do it explicitly here.
            let candidate = Path::new(target);
            let joined = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                project_dir.join(candidate)
            };
            pacquet_fs::lexical_normalize(&joined)
        }
        ImporterDepVersion::File(_) => {
            // Injected workspace dep that didn't dedupe back to
            // `link:` — the importer entry references a virtual-store
            // slot keyed by `(importer_key, file:<payload>)`. Route
            // through `resolved_key` so the layout's GVS-suffix map
            // sees the same key the snapshot writer used.
            let dep_key =
                spec.version.resolved_key(name).expect("File arm always produces a resolved_key");
            layout.slot_dir(&dep_key).join("node_modules").join(name_str)
        }
    }
}

/// Build the `<alias → resolved-target>` map a dedupe pass needs
/// for a single importer (always the root). Mirrors the per-importer
/// filters in [`collect_resolved_entries`] so the map only contains
/// aliases that would have been symlinked, matching pnpm's
/// `readLinkedDeps(rootProject.modulesDir)` — pnpm reads the root's
/// `node_modules/` after `linkDirectDepsOfProject` runs, which by
/// then contains exactly the entries that survived its own
/// skipped / link-only filters.
fn collect_resolved_targets(
    layout: &VirtualStoreLayout,
    project_snapshot: &ProjectSnapshot,
    project_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
) -> BTreeMap<String, PathBuf> {
    collect_resolved_entries(
        layout,
        project_snapshot,
        project_dir,
        dependency_groups,
        skipped,
        link_only,
    )
    .into_iter()
    .map(|entry| (entry.name_str, entry.target))
    .collect()
}

#[cfg(test)]
mod tests;
