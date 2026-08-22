//! Whether the manifests on disk still agree with the lockfile.

use super::{
    Config, DependencyGroup, FileMtime, ImporterDepVersion, Lockfile, OptimisticRepeatInstallCheck,
    PackageManifest, Path, PathBuf, ProjectSnapshot, WorkspaceState, file_mtime,
    modified_at_or_after, mtime_ms,
};

/// One project manifest's stat outcome, paired with the inputs the
/// content re-check needs.
pub(crate) struct ManifestStat<'a> {
    pub(crate) root_dir: &'a Path,
    pub(crate) manifest: &'a PackageManifest,
    pub(crate) mtime: FileMtime,
}

/// The modified-manifests branch: the lockfile-equality assertion plus
/// the wanted-lockfile up-to-date check (settings drift, per-importer
/// specifier match, linked-package freshness) for every project whose
/// manifest is newer than the last validation. `Err` carries the
/// `Decision::Skipped` reason.
///
/// When `pnpm-lock.yaml` is absent, the current lockfile stands in as
/// the wanted one (see the lockfile gate in
/// [`crate::optimistic_repeat_install::check_optimistic_repeat_install`]); `Ok(Some(_))` then carries the
/// loaded current lockfile so the caller can regenerate
/// `pnpm-lock.yaml` from it without a second read.
pub(crate) fn modified_manifests_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    modified: &[&ManifestStat<'_>],
    dedupe_peers: bool,
) -> Result<Option<Lockfile>, &'static str> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        is_workspace_install,
        lockfile,
        catalogs,
        ..
    } = check;
    let mut loaded_current: Option<Lockfile> = None;
    let mut wanted_is_current = false;
    let lockfile = lockfile.get().map_err(|_| "the wanted lockfile cannot be read or parsed")?;
    let (wanted, wanted_mtime): (&Lockfile, FileMtime) = if let Some(wanted) = lockfile {
        let Some(mtime) = file_mtime(&workspace_root.join(config.wanted_lockfile_name())) else {
            return Err(
                "a manifest is newer than the last validation and the wanted lockfile cannot be stat'd",
            );
        };
        (wanted, mtime)
    } else {
        let current_path = config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        let Some(mtime) = file_mtime(&current_path) else {
            return Err("a manifest is newer than the last validation and no lockfile is loaded");
        };
        let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?
            .ok_or("a manifest is newer than the last validation and no lockfile is loaded")?;
        wanted_is_current = true;
        (&*loaded_current.insert(current), mtime)
    };

    // Decide which modified projects need the full content check, and
    // whether the wanted lockfile must be compared against the current
    // one (`<virtual_store_dir>/lock.yaml`).
    let to_check: &[&ManifestStat<'_>] = if wanted_is_current {
        // The wanted lockfile IS the current one — there's no second
        // lockfile to assert equality against, and the mtime
        // short-circuits below compare the two lockfile files, so they
        // don't apply. Every modified project gets the content check.
        modified
    } else if is_workspace_install {
        // Workspace branch: a wanted lockfile newer than the last
        // validation must equal what the previous install materialized.
        if modified_at_or_after(wanted_mtime, state.last_validated_timestamp) {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        modified
    } else {
        // Single-project branch keys off the lockfile mtimes instead of
        // `lastValidatedTimestamp`.
        let current_mtime_ms =
            mtime_ms(&config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME));
        if let Some(current_mtime_ms) = current_mtime_ms
            && modified_at_or_after(wanted_mtime, current_mtime_ms)
        {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        let root = modified.first().expect("modified-manifests branch requires a modified project");
        if modified_at_or_after(root.mtime, wanted_mtime.ms) {
            modified
        } else if current_mtime_ms.is_some() {
            // "The manifest file is not newer than the lockfile.
            // Exiting check."
            &[]
        } else if !wanted.is_empty() {
            // RUN_CHECK_DEPS_NO_DEPS: the lockfile requires
            // dependencies but nothing was ever installed.
            return Err("the lockfile requires dependencies but none were installed");
        } else {
            &[]
        }
    };

    if to_check.is_empty() {
        return Ok(loaded_current);
    }

    let parsed_overrides = crate::install::parse_config_overrides(config, catalogs)
        .map_err(|_| "pnpm.overrides cannot be parsed")?;
    if let Err(error) = crate::install::check_lockfile_settings_drift(
        wanted,
        config,
        catalogs,
        crate::install::CheckLockfileSettingsDriftOptions {
            parsed_overrides: parsed_overrides.as_deref(),
            // `pnpmfileChecksum` needs no comparison here: reaching this
            // point means `pnpmfiles_modified_since` already proved the
            // pnpmfile list and contents are what the install that wrote
            // this lockfile saw. Computing the checksum instead would cost
            // a Node worker on the path that exists to avoid starting one.
            pnpmfile_checksum: pnpm_lockfile::PnpmfileChecksumCheck::Skip,
            dedupe_peers,
        },
    ) {
        tracing::debug!(target: "pacquet::install", %error, "repeat-install content check: lockfile settings drift");
        return Err("a lockfile setting drifted from the current configuration");
    }

    let linked_ctx = LinkedPackagesContext::new(config, project_manifests);
    let ignored_optional_matcher = pnpm_config::matcher::create_matcher(
        config.ignored_optional_dependencies.as_deref().unwrap_or_default(),
    );
    for project in to_check {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, project.root_dir);
        if let Err(error) = crate::install::check_importer_satisfies(
            wanted,
            project.manifest,
            &importer_id,
            config,
            &ignored_optional_matcher,
            parsed_overrides.as_deref(),
        ) {
            tracing::debug!(target: "pacquet::install", %error, importer_id, "repeat-install content check: manifest no longer satisfied");
            return Err("a modified manifest is no longer satisfied by the lockfile");
        }
        let Some(importer) = wanted.importers.get(&importer_id) else {
            return Err("a modified project has no importer entry in the lockfile");
        };
        if !linked_packages_are_up_to_date(
            &linked_ctx,
            project.root_dir,
            project.manifest,
            importer,
        ) {
            return Err("a linked package is out of date");
        }
    }
    Ok(loaded_current)
}

/// Assert the wanted lockfile equals the current one: with no current
/// lockfile every importer of the wanted one must be dependency-free
/// (`RUN_CHECK_DEPS_NO_DEPS`); otherwise the two parsed lockfiles must
/// be equal (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
pub(crate) fn assert_wanted_lockfile_equals_current(
    wanted: &Lockfile,
    config: &Config,
) -> Result<(), &'static str> {
    let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
        .map_err(|_| "the current lockfile cannot be loaded")?;
    match current {
        None => {
            let any_deps = wanted.importers.values().any(|snapshot| {
                snapshot
                    .dependencies_by_groups([
                        DependencyGroup::Prod,
                        DependencyGroup::Dev,
                        DependencyGroup::Optional,
                    ])
                    .next()
                    .is_some()
            });
            if any_deps {
                Err("the lockfile requires dependencies but none were installed")
            } else {
                Ok(())
            }
        }
        Some(current) => {
            if &current == wanted {
                Ok(())
            } else {
                Err("the installed dependencies are not up to date with the lockfile")
            }
        }
    }
}

/// Shared lookups for [`linked_packages_are_up_to_date`], built once
/// per content check.
pub(crate) struct LinkedPackagesContext<'a> {
    pub(crate) link_workspace_packages: bool,
    pub(crate) manifests_by_dir: std::collections::HashMap<&'a Path, &'a PackageManifest>,
    /// `name → version → root_dir` over the workspace's projects.
    pub(crate) workspace_packages:
        std::collections::HashMap<String, std::collections::HashMap<String, &'a Path>>,
}

/// Verify that linked packages are up to date: every importer
/// dependency that resolved to a workspace link must still link under
/// today's manifest spec, and every one that resolved to the registry
/// must not have become linkable. The local-file-dependency freshness
/// branch (a `file:` directory specifier) is not handled here — those
/// entries conservatively report "not up to date" so the full install
/// path re-evaluates them.
pub(crate) fn linked_packages_are_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    manifest: &PackageManifest,
    snapshot: &ProjectSnapshot,
) -> bool {
    const GROUPS: [(DependencyGroup, &str); 3] = [
        (DependencyGroup::Optional, "optionalDependencies"),
        (DependencyGroup::Prod, "dependencies"),
        (DependencyGroup::Dev, "devDependencies"),
    ];
    for (group, manifest_field) in GROUPS {
        let Some(lockfile_deps) = snapshot.get_map_by_group(group) else {
            continue;
        };
        let Some(manifest_deps) =
            manifest.value().get(manifest_field).and_then(|value| value.as_object())
        else {
            continue;
        };
        for (dep_name, dep) in lockfile_deps {
            let dep_name = dep_name.to_string();
            let Some(current_spec) = manifest_deps.get(&dep_name).and_then(|v| v.as_str()) else {
                continue;
            };
            if ref_is_local_directory(&dep.specifier) {
                // A `file:` specifier that resolved to `link:` (e.g. an
                // injected self-reference) is a local link with no
                // `packages:` entry — up to date by construction.
                if matches!(dep.version, ImporterDepVersion::Link(_)) {
                    continue;
                }
                return false;
            }
            let link_target = dep.version.as_link_target();
            let is_linked = link_target.is_some();
            if is_linked
                && (current_spec.starts_with("link:")
                    || current_spec.starts_with("file:")
                    || current_spec.starts_with("workspace:."))
            {
                continue;
            }
            // A linked dependency whose spec is a distribution tag is
            // considered up to date to skip full resolution.
            if is_linked && spec_is_distribution_tag(current_spec) {
                continue;
            }
            let linked_dir: Option<std::borrow::Cow<'_, Path>> = match link_target {
                Some(target) => Some(std::borrow::Cow::Owned(project_dir.join(target))),
                None => dep
                    .version
                    .as_regular()
                    .map(std::string::ToString::to_string)
                    .and_then(|version| ctx.workspace_packages.get(&dep_name)?.get(&version))
                    .map(|dir| std::borrow::Cow::Borrowed(*dir)),
            };
            let Some(linked_dir) = linked_dir else {
                continue;
            };
            if !ctx.link_workspace_packages && !current_spec.starts_with("workspace:") {
                // A linkable dir exists, but nothing requests linking it.
                continue;
            }
            let available_range = version_range_of_spec(current_spec);
            let local_package_satisfies_range = matches!(available_range, "*" | "^" | "~")
                || ctx
                    .linked_version(&linked_dir)
                    .is_some_and(|version| semver_satisfies_loosely(&version, available_range));
            if is_linked != local_package_satisfies_range {
                return false;
            }
        }
    }
    true
}

/// Whether a specifier points at a local directory: a `file:`
/// specifier that is not a tarball.
pub(crate) fn ref_is_local_directory(specifier: &str) -> bool {
    specifier.starts_with("file:")
        && !(specifier.ends_with(".tgz")
            || specifier.ends_with(".tar.gz")
            || specifier.ends_with(".tar"))
}

/// Whether a bare specifier is an npm distribution tag (`latest`,
/// `beta`, ...): anything that doesn't parse as a semver range and
/// contains only characters a tag name may carry. Protocol-ish specs
/// (`workspace:^1.0.0`, `npm:foo@1`) contain `:`/`@`/`/` and therefore
/// never match.
pub(crate) fn spec_is_distribution_tag(spec: &str) -> bool {
    !spec.is_empty()
        && spec.parse::<node_semver::Range>().is_err()
        && spec.chars().all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
}

/// Strip the `workspace:` / `npm:` envelope so the remainder can be
/// compared as a semver range.
pub(crate) fn version_range_of_spec(spec: &str) -> &str {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        return rest;
    }
    if let Some(rest) = spec.strip_prefix("npm:") {
        // `npm:<alias>@<range>` — the `@` search starts at index 1 so a
        // leading scope `@` isn't mistaken for the separator.
        return match rest.get(1..).and_then(|tail| tail.find('@')) {
            Some(at) => {
                let range = &rest[at + 2..];
                if range.is_empty() { "*" } else { range }
            }
            None => "*",
        };
    }
    spec
}

/// `semver.satisfies(version, range, { loose: true })` — a version or
/// range that doesn't parse fails the match.
pub(crate) fn semver_satisfies_loosely(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<node_semver::Version>() else { return false };
    let Ok(range) = range.parse::<node_semver::Range>() else { return false };
    range.satisfies(&version)
}

/// Stat every project's `package.json`. `None` on any stat failure —
/// "can't prove freshness, fall through".
pub(crate) fn stat_manifests<'a>(
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
) -> Option<Vec<ManifestStat<'a>>> {
    project_manifests
        .iter()
        .map(|(root_dir, manifest)| {
            file_mtime(manifest.path()).map(|mtime| ManifestStat {
                root_dir: root_dir.as_path(),
                manifest,
                mtime,
            })
        })
        .collect()
}
