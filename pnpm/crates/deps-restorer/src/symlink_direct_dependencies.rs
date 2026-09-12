mod report;
use report::emit_root_added;

mod resolve;
use resolve::{ResolvedEntry, collect_resolved_entries, collect_resolved_targets};

use crate::{SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, symlink_package};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{LinkBinsError, LinkBinsOptions};
use pnpm_config::Config;
use pnpm_lockfile::{ImporterDepVersion, PackageKey, PackageMetadata, PkgName, ProjectSnapshot};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
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
///   importer's `rootDir` as the event prefix (a per-project emit).
///
/// The virtual store dir (`config.virtual_store_dir`) stays singular
/// across the install — only the per-project `node_modules/` and its
/// symlinks fan out. By default `pnpm_config::default_virtual_store_dir`
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
    /// Per-package metadata from the lockfile. Non-registry packages carry
    /// their manifest version here because their importer version slot is a
    /// URL or path rather than the package's semantic version.
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub dependency_groups: DependencyGroupList,
    /// Workspace root. For a single-project install this is the
    /// directory containing the user's `package.json`; for a real
    /// workspace it's the directory containing `pnpm-workspace.yaml`.
    /// Same value as the `lockfileDir` used for
    /// `pnpm:stage` / `pnpm:summary` events.
    pub workspace_root: &'a Path,
    /// Snapshots the installability pass marked optional+incompatible.
    /// A direct dep whose resolved snapshot key is in this set is
    /// omitted from `node_modules/<name>` (no symlink, no
    /// `pnpm:root added` event, no bin linking).
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
    /// In the hoisted branch this runs after
    /// `linkHoistedModules` with the direct-dependency map filtered to
    /// only `link:`-shaped entries.
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

    /// Importer ids whose project directories the caller *knows* —
    /// they came from the install's own project list (the programmatic
    /// API's in-memory projects, or `pnpm-workspace.yaml` discovery),
    /// not from parsed lockfile input. These bypass
    /// [`validate_importer_id`]: a declared project may legitimately
    /// live outside the lockfile dir (importer id `..` or `../foo`) —
    /// Bit's capsule installs do exactly that, and pnpm v11 linked
    /// such importers without complaint. Ids *not* in this set keep
    /// the strict malformed-lockfile rejection.
    pub trusted_importer_ids: Option<&'a HashSet<String>>,

    /// [`crate::shim_link_options`] output — threaded into the
    /// per-importer `.bin` shim pass.
    pub link_options: &'a LinkBinsOptions,

    /// Parsed manifests recovered from the store-index prefetch
    /// ([`crate::PackageManifests`]), when the caller has them. Feeds
    /// [`crate::link_direct_dep_bins_prefetched`] so an importer's bin
    /// pass reads no `package.json` for a prefetched dep; `None` keeps
    /// every dep on the disk-read fallback.
    pub package_manifests: Option<&'a crate::PackageManifests>,

    /// Per-snapshot `requiresBuild` flags from the same prefetch,
    /// gating [`Self::package_manifests`] — see
    /// [`crate::link_direct_dep_bins_prefetched`].
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
}

/// Error type of [`SymlinkDirectDependencies`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum SymlinkDirectDependenciesError {
    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    /// A lockfile importer key that would escape the workspace root.
    /// The lockfile spec uses POSIX relative paths for importer
    /// keys (e.g. `packages/web`); a key that is absolute, contains
    /// `..` traversal, or carries a Windows drive prefix is treated
    /// as a malformed lockfile so we don't end up creating
    /// `node_modules` outside the workspace. The importer keys a
    /// conforming lockfile writes are always relative POSIX paths
    /// under the workspace root, so this check never rejects valid
    /// input.
    #[display("Refusing to install importer with unsafe path key {importer_id:?}")]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH),
        help(
            "Importer keys in pnpm-lock.yaml must be POSIX paths relative to the workspace root (e.g. `packages/web`). Absolute paths, drive prefixes, and `..` components are rejected."
        )
    )]
    UnsafeImporterPath {
        #[error(not(source))]
        importer_id: String,
    },

    /// Surfaces a per-package symlink failure (e.g. permission denied,
    /// disk full, an existing non-symlink file).
    #[display("Failed to symlink {name:?} for importer {importer_id:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_SYMLINK_FAILED))]
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
        // Collect once so the same group order can drive every importer.
        let dependency_groups: Vec<DependencyGroup> = self.dependency_groups.into_iter().collect();
        ImporterPass {
            config: self.config,
            layout: self.layout,
            importers: self.importers,
            packages: self.packages,
            dependency_groups,
            workspace_root: self.workspace_root,
            skipped: self.skipped,
            link_only: self.link_only,
            public_hoist_targets: self.public_hoist_targets,
            trusted_importer_ids: self.trusted_importer_ids,
            link_options: self.link_options,
            // One bin lookup for the whole pass: the `hasBin` gate and
            // the shim probe memo are importer-invariant.
            bin_lookup: crate::PrefetchedBinLookup::new(
                self.packages,
                self.package_manifests,
                self.requires_build_by_snapshot,
            ),
        }
        .run::<Reporter>()
    }
}

/// [`SymlinkDirectDependencies`] with its group list collected and its
/// bin lookup built, which every importer's pass reads.
struct ImporterPass<'a> {
    config: &'static Config,
    layout: &'a VirtualStoreLayout,
    importers: &'a HashMap<String, ProjectSnapshot>,
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    dependency_groups: Vec<DependencyGroup>,
    workspace_root: &'a Path,
    skipped: &'a SkippedSnapshots,
    link_only: bool,
    public_hoist_targets: Option<&'a BTreeMap<String, PathBuf>>,
    trusted_importer_ids: Option<&'a HashSet<String>>,
    link_options: &'a LinkBinsOptions,
    bin_lookup: crate::PrefetchedBinLookup<'a>,
}

impl ImporterPass<'_> {
    fn run<Reporter: self::Reporter>(&self) -> Result<(), SymlinkDirectDependenciesError> {
        // Each importer's modules dir is `<importer_root>/<modules_dir_basename>`.
        // The `modulesDir` setting is a directory name (a single
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
            self.config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));

        // Sorted so the fallible upfront validation below rejects a
        // hostile lockfile on a deterministic importer. `pnpm:root`
        // event order is not pinned — the per-importer work runs on
        // rayon, matching pnpm's `Promise.all` over importers — so
        // consumers key events off their `prefix`, never their order.
        let mut keys: Vec<&str> = self.importers.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let root_targets = self.root_dedupe_targets(&keys);
        self.validate_importer_ids(&keys)?;

        // One rayon task per importer, mirroring pnpm's `Promise.all`
        // over `linkDirectDeps`' projects: each importer's symlink and
        // bin work is independent (dedupe compares against the *plan*
        // in `root_targets`, not the root importer's on-disk state), and
        // a serial walk would insert a fork-join barrier per importer
        // between the filesystem batches.
        let task_groups = importer_task_groups(self.workspace_root, keys);
        task_groups.par_iter().try_for_each(|group| {
            group.iter().try_for_each(|importer_id| {
                self.link_importer::<Reporter>(importer_id, modules_dir_name, root_targets.as_ref())
            })
        })
    }

    /// `dedupeDirectDeps` short-circuits when there is no root importer
    /// or only one importer total — there's nothing to dedupe against.
    fn root_dedupe_targets(&self, keys: &[&str]) -> Option<BTreeMap<String, PathBuf>> {
        let dedupe =
            self.config.dedupe_direct_deps && self.importers.contains_key(".") && keys.len() > 1;
        dedupe.then(|| {
            root_dedupe_targets(
                self.layout,
                &self.importers["."],
                &importer_root_dir(self.workspace_root, "."),
                &self.dependency_groups,
                self.skipped,
                self.link_only,
                self.public_hoist_targets,
            )
        })
    }

    /// Reject importer keys that would escape the workspace root. A
    /// malformed (or hostile) lockfile could otherwise make `Path::join`
    /// create `node_modules` outside the workspace — `Path::join`
    /// discards the base when the RHS is absolute, and `..` components
    /// are otherwise permitted. Importer ids the caller declared as
    /// projects (see [`SymlinkDirectDependencies::trusted_importer_ids`])
    /// skip the check — an explicitly-configured project may live
    /// outside the lockfile dir. Validated before any importer links, so
    /// a rejected lockfile writes nothing.
    fn validate_importer_ids(&self, keys: &[&str]) -> Result<(), SymlinkDirectDependenciesError> {
        for importer_id in keys {
            if !self.trusted_importer_ids.is_some_and(|trusted| trusted.contains(*importer_id)) {
                validate_importer_id(importer_id)?;
            }
        }
        Ok(())
    }

    fn link_importer<Reporter: self::Reporter>(
        &self,
        importer_id: &str,
        modules_dir_name: &OsStr,
        root_targets: Option<&BTreeMap<String, PathBuf>>,
    ) -> Result<(), SymlinkDirectDependenciesError> {
        // Safe: the task groups were built from `importers.keys()`.
        let project_snapshot = &self.importers[importer_id];
        let project_dir = importer_root_dir(self.workspace_root, importer_id);
        let modules_dir = project_dir.join(modules_dir_name);

        // Only non-root importers get deduped against root: the
        // root project is linked unfiltered, then each sibling's
        // list is trimmed against what root links.
        let dedupe_against = root_targets.filter(|_| importer_id != ".");

        link_one_importer::<Reporter>(
            importer_id,
            self.layout,
            project_snapshot,
            self.packages,
            &project_dir,
            &modules_dir,
            self.dependency_groups.iter().copied(),
            self.skipped,
            self.link_only,
            dedupe_against,
            self.config.symlink,
            self.link_options,
            &self.bin_lookup,
        )
    }
}

/// What the root importer resolves each alias to, for the
/// `dedupeDirectDeps` comparison.
///
/// Publicly-hoisted aliases are folded in alongside root's direct deps.
/// Pnpm's `linkDirectDepsAndDedupe` reads root's `node_modules/` after
/// the hoist pass populates it, so its dedupe naturally covers both
/// kinds; pacquet runs hoist *after* this step, so the caller
/// pre-computes the hoist plan and feeds the public-side targets here.
/// Direct deps win on collision — a root direct dep won't be silently
/// overwritten by a hoist plan entry that resolves to a different slot.
fn root_dedupe_targets(
    layout: &VirtualStoreLayout,
    root_snapshot: &ProjectSnapshot,
    root_project_dir: &Path,
    dependency_groups: &[DependencyGroup],
    skipped: &SkippedSnapshots,
    link_only: bool,
    public_hoist_targets: Option<&BTreeMap<String, PathBuf>>,
) -> BTreeMap<String, PathBuf> {
    let mut targets = collect_resolved_targets(
        layout,
        root_snapshot,
        root_project_dir,
        dependency_groups.iter().copied(),
        skipped,
        link_only,
    );
    for (alias, target) in public_hoist_targets.into_iter().flatten() {
        targets.entry(alias.clone()).or_insert_with(|| target.clone());
    }
    targets
}

/// Partition validated importer keys into the concurrency-safe task
/// groups the parallel link pass runs.
///
/// Distinct keys may alias one directory through the filesystem's own
/// name folding — case-insensitivity, Unicode normalization (APFS),
/// Windows short names and trailing dots — and a real pnpm lockfile
/// can't produce them (project discovery would have collapsed the
/// directories), but a hostile lockfile can, and two tasks mutating
/// one `node_modules` would race. Rather than enumerate the folding
/// rules, ask the filesystem: importers whose project dirs
/// canonicalize to one path share a group, in the caller's (sorted)
/// order, keeping the serial pass's deterministic last-writer outcome,
/// while distinct projects pay one read-only `canonicalize` each. A
/// project dir that doesn't exist yet has nothing on disk to
/// canonicalize against — and no string transform can decide which
/// not-yet-created names the filesystem will later fold together — so
/// every canonicalization failure lands in one shared serial group.
/// That costs nothing real: a genuine project's directory always
/// exists by this point (its manifest was read during project
/// discovery), so the shared group only ever collects the phantom
/// importers of a malformed lockfile.
fn importer_task_groups<'a>(workspace_root: &Path, keys: Vec<&'a str>) -> Vec<Vec<&'a str>> {
    let mut task_groups: BTreeMap<PathBuf, Vec<&'a str>> = BTreeMap::new();
    let mut unresolved: Vec<&'a str> = Vec::new();
    for importer_id in keys {
        let project_dir = importer_root_dir(workspace_root, importer_id);
        match std::fs::canonicalize(&project_dir) {
            Ok(canonical) => task_groups.entry(canonical).or_default().push(importer_id),
            Err(_) => unresolved.push(importer_id),
        }
    }
    let mut task_groups: Vec<Vec<&'a str>> = task_groups.into_values().collect();
    if !unresolved.is_empty() {
        task_groups.push(unresolved);
    }
    task_groups
}

/// Reject importer keys that would resolve outside the workspace root.
///
/// Pnpm's lockfile spec writes importer keys as POSIX paths relative
/// to the workspace root (`.` for the root, `packages/web` for a
/// subproject). Anything else — an absolute POSIX path, a Windows
/// drive prefix, a `..` segment — is either malformed or hostile, so
/// surface it as a typed error rather than silently letting
/// `Path::join` produce an off-workspace path.
/// Reject a lockfile importer key that cannot be safely joined onto the
/// lockfile dir: absolute paths, Windows drive prefixes, backslash
/// separators, and `..` traversal segments. `.` (the root importer) and any
/// ordinary POSIX-relative sub-path are accepted. Callers that turn an
/// importer key into an on-disk path — via [`importer_root_dir`] — must run
/// this first when the key comes from an untrusted lockfile.
pub fn validate_importer_id(importer_id: &str) -> Result<(), SymlinkDirectDependenciesError> {
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
    // Any `..` segment (mirrors `path::Component::ParentDir` rejection),
    // plus the non-canonical forms `.` and the empty segment (`a//b`,
    // a trailing `/`). Pnpm only ever writes canonical relative keys,
    // and a non-canonical key is not just malformed: two distinct keys
    // like `packages/a` and `packages/./a` resolve to one directory,
    // and the importers now link concurrently — aliased keys would
    // race their symlink and `.bin` writes against each other.
    for segment in importer_id.split('/') {
        if segment == ".." || segment == "." || segment.is_empty() {
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
    let unique_deps = dependency_groups
        .into_iter()
        .filter(|group| !matches!(group, DependencyGroup::Peer))
        .flat_map(|group| snapshot.get_map_by_group(group).into_iter().flatten())
        .filter(|(name, _)| seen.insert(*name));
    unique_deps
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
/// The on-disk root of the project a lockfile importer ID names, relative
/// to `workspace_root` (which is normally the lockfile directory).
#[must_use]
pub fn importer_root_dir(workspace_root: &Path, importer_id: &str) -> PathBuf {
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

fn link_resolved_entry<Reporter: self::Reporter>(
    entry: &ResolvedEntry<'_>,
    importer_id: &str,
    modules_dir: &Path,
    symlink: bool,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    prefix: &str,
) -> Result<(), SymlinkDirectDependenciesError> {
    let ResolvedEntry { name_str, target, .. } = entry;

    if symlink {
        let outcome = symlink_package(target, &modules_dir.join(name_str)).map_err(|source| {
            SymlinkDirectDependenciesError::SymlinkPackage {
                importer_id: importer_id.to_string(),
                name: name_str.clone(),
                source,
            }
        })?;

        if outcome.reused {
            return Ok(());
        }
    }

    emit_root_added::<Reporter>(entry, packages, prefix);
    Ok(())
}

// Absolute target paths make lexical equality sufficient; deduped entries also lose their bins.
fn dedupe_resolved_entries<'a>(
    entries: Vec<ResolvedEntry<'a>>,
    dedupe_against: Option<&BTreeMap<String, PathBuf>>,
) -> Vec<ResolvedEntry<'a>> {
    if let Some(root_targets) = dedupe_against {
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
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent inputs; bundling them into a struct would not improve clarity"
)]
fn link_one_importer<Reporter: self::Reporter>(
    importer_id: &str,
    layout: &VirtualStoreLayout,
    project_snapshot: &ProjectSnapshot,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    project_dir: &Path,
    modules_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
    dedupe_against: Option<&BTreeMap<String, PathBuf>>,
    symlink: bool,
    link_options: &LinkBinsOptions,
    bin_lookup: &crate::PrefetchedBinLookup<'_>,
) -> Result<(), SymlinkDirectDependenciesError> {
    let entries = collect_resolved_entries(
        layout,
        project_snapshot,
        project_dir,
        dependency_groups,
        skipped,
        link_only,
    );

    let entries = dedupe_resolved_entries(entries, dedupe_against);

    // `prefix` for the `pnpm:root` envelope: the project's `rootDir`
    // so the reporter can scope progress to the right project —
    // `lockfileDir` is reserved for the install-wide stage / summary
    // events.
    let prefix = project_dir.to_string_lossy().into_owned();

    // `try_for_each` short-circuits on the first error and returns it
    // to the caller. The full result collection forces every task to
    // settle before we surface a single error.
    entries.par_iter().try_for_each(|entry| -> Result<(), SymlinkDirectDependenciesError> {
        link_resolved_entry::<Reporter>(entry, importer_id, modules_dir, symlink, packages, &prefix)
    })?;

    // After the symlinks exist, walk them to discover each
    // direct dep's `package.json` and link declared bins into
    // `<modules_dir>/.bin`. Each entry's `target` is the symlink's
    // destination, so the bin pass gets the resolved location for
    // free.
    if symlink {
        let deps: Vec<crate::PrefetchedDepBin> = entries
            .iter()
            .map(|entry| {
                let snapshot_key = entry.spec.version.resolved_key(entry.name);
                (entry.name_str.clone(), entry.target.clone(), snapshot_key)
            })
            .collect();
        crate::link_direct_dep_bins_prefetched(modules_dir, &deps, bin_lookup, link_options)
            .map_err(SymlinkDirectDependenciesError::LinkBins)?;
    } else {
        let locations: Vec<PathBuf> = entries.iter().map(|entry| entry.target.clone()).collect();
        crate::link_direct_dep_bins_from_locations(modules_dir, &locations, link_options)
            .map_err(SymlinkDirectDependenciesError::LinkBins)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
