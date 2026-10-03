//! Migration of the global packages pnpm 10 left behind, run by `update -g`.
//!
//! pnpm 10 installed every global package into one project,
//! `<global-dir>/5`, and linked their bins straight into the pnpm home.
//! The current layout keeps each package in its own group under
//! `<global-dir>/v11` and links bins into `<pnpm-home>/bin`, so after an
//! upgrade the packages of the old project are neither on `PATH` nor
//! listed by `list -g`.

use super::{
    super::reporter::EventFilter,
    GlobalInstallTarget, check_bin_dir, global_dirs,
    selectors::{is_pnpm_cli_dependency, resolve_local_param},
    warn_global,
};
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, Host, default_pnpm_home_dir};
use pnpm_fs::{
    is_subdir, lexical_normalize, realpath_missing, remove_dir_all_with_retry,
    remove_file_with_retry,
};
use pnpm_global::{
    GlobalPackageInfo, clean_orphaned_install_dirs, get_global_package_details, get_installed_bins,
    read_direct_dependencies, scan_global_packages,
};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

const LEGACY_GLOBAL_LAYOUT: &str = "5";

/// A generated shim stays far below this; a larger file is not one.
const MAX_SHIM_BYTES: u64 = 64 * 1024;

/// The files pnpm 10 wrote for one bin: the sh shim, and on Windows the
/// `.cmd` and `.ps1` shims beside it, or a hard-linked `.exe`.
const LEGACY_BIN_EXTENSIONS: [&str; 4] = ["", ".cmd", ".ps1", ".exe"];

/// The global project of the previous layout, next to the current one.
pub(super) struct LegacyGlobalLayout {
    dir: PathBuf,
    /// The direct dependencies its manifest records, the pnpm CLI included:
    /// its bins are cleaned up like any other, while installing it into
    /// the current layout is `pnpm setup`'s job.
    dependencies: Vec<(String, String)>,
}

/// One package to migrate. Messages name the alias: the selector can carry
/// the credentials of a tarball or git URL.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MigrationSelector {
    pub(super) alias: String,
    pub(super) selector: String,
}

/// What the current groups install: every alias they declare, and the
/// subset whose package files are on disk.
struct CurrentGroups {
    declared: BTreeSet<String>,
    materialized: BTreeSet<String>,
}

impl CurrentGroups {
    fn scan(global_pkg_dir: &Path) -> miette::Result<Self> {
        let groups = scan_global_packages(global_pkg_dir)
            .into_diagnostic()
            .wrap_err("scan global packages")?;
        let declared = groups
            .iter()
            .flat_map(|pkg| pkg.dependencies.iter().map(|(alias, _)| alias.clone()))
            .collect();
        let materialized = groups
            .iter()
            .flat_map(|pkg| get_global_package_details(pkg).into_iter().map(|pkg| pkg.alias))
            .collect();
        Ok(Self { declared, materialized })
    }
}

impl LegacyGlobalLayout {
    /// The dependencies other than the pnpm CLI.
    fn packages_to_migrate(&self) -> impl Iterator<Item = &(String, String)> {
        self.dependencies
            .iter()
            .filter(|(alias, spec)| !is_pnpm_cli_dependency(alias, Some(spec)))
    }

    /// The dependencies a current group declares without having their
    /// files. Until they are installed again, the previous project holds
    /// the only copy.
    fn aliases_awaiting_restore(&self, current: &CurrentGroups) -> Vec<&str> {
        self.packages_to_migrate()
            .map(|(alias, _)| alias.as_str())
            .filter(|alias| {
                current.declared.contains(*alias) && !current.materialized.contains(*alias)
            })
            .collect()
    }

    /// The project next to `global_pkg_dir`, if its manifest is still there.
    /// A manifest that does not parse is an error rather than an empty
    /// project: an empty one reads as "nothing left to migrate" and is
    /// deleted.
    pub(super) fn find(global_pkg_dir: &Path) -> miette::Result<Option<Self>> {
        let Some(dir) = global_pkg_dir
            .parent()
            .map(|parent| parent.join(LEGACY_GLOBAL_LAYOUT))
        else {
            return Ok(None);
        };
        if safe_read_package_json_from_dir(&dir).map_err(miette::Report::new)?.is_none() {
            return Ok(None);
        }
        Ok(Some(Self { dir: dir.clone(), dependencies: read_direct_dependencies(&dir) }))
    }

    /// The dependencies no current group installs, each with its `add -g`
    /// selector. A local path is anchored at the previous project, which
    /// is where pnpm 10 recorded it relative to.
    pub(super) fn selectors_to_migrate(
        &self,
        installed_aliases: &BTreeSet<String>,
    ) -> Vec<MigrationSelector> {
        self.packages_to_migrate()
            .filter(|(alias, _)| !installed_aliases.contains(alias))
            .map(|(alias, spec)| MigrationSelector {
                alias: alias.clone(),
                selector: format!("{alias}@{}", resolve_local_param(spec, &self.dir)),
            })
            .collect()
    }

    /// Delete the bins pnpm 10 linked into the pnpm home for its packages,
    /// then the project. A bin that cannot be removed keeps the project, so
    /// the next `update -g` can retry.
    fn remove<Reporter: self::Reporter>(self) -> miette::Result<()> {
        if !self.remove_home_bins::<Reporter>() {
            let dir = self.dir.display();
            warn_global::<Reporter>(&format!(
                "Kept {dir} because a bin pnpm 10 linked into the pnpm home could not be removed. \
                 The next \"pnpm update -g\" retries.",
            ));
            return Ok(());
        }
        remove_dir_all_with_retry(&self.dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("remove {}", self.dir.display()))?;
        info_global::<Reporter>(&format!("Removed {}", self.dir.display()));
        Ok(())
    }

    /// Returns whether every bin of the previous project was removed.
    fn remove_home_bins<Reporter: self::Reporter>(&self) -> bool {
        let Some(pnpm_home) = default_pnpm_home_dir::<Host>() else { return true };
        let info = GlobalPackageInfo {
            hash: String::new(),
            install_dir: self.dir.clone(),
            dependencies: self.dependencies.clone(),
        };
        let files = match legacy_home_bin_files(&info, &pnpm_home) {
            Ok(files) => files,
            Err(error) => {
                let dir = self.dir.display();
                warn_global::<Reporter>(&format!(
                    "Failed to read the bins linked from {dir}: {error}",
                ));
                return false;
            }
        };
        let mut every_bin_removed = true;
        for file in files {
            if let Err(error) = remove_file_with_retry(&file) {
                every_bin_removed = false;
                let file = file.display();
                warn_global::<Reporter>(&format!("Failed to remove {file}: {error}"));
            }
        }
        every_bin_removed
    }
}

fn legacy_home_bin_files(
    info: &GlobalPackageInfo,
    pnpm_home: &Path,
) -> miette::Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for bin in get_installed_bins(info).map_err(miette::Report::new)? {
        let bin_path = pnpm_home.join(&bin.name);
        files.extend(
            legacy_bin_files(&bin_path, &info.install_dir, Some(&bin.path))
                .into_diagnostic()
                .wrap_err_with(|| format!("inspect {}", bin_path.display()))?,
        );
    }
    Ok(files)
}

/// The files pnpm 10 wrote for the bin at `bin_path` that are still a link
/// or shim into `legacy_dir`, or a hard link to `target`. Each file is judged
/// on its own. Hard links are recognized only when the target resolves inside
/// `legacy_dir`; other files at these paths are kept.
fn legacy_bin_files(
    bin_path: &Path,
    legacy_dir: &Path,
    target: Option<&Path>,
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for extension in LEGACY_BIN_EXTENSIONS {
        let mut file = bin_path.as_os_str().to_owned();
        file.push(extension);
        let file = PathBuf::from(file);
        if is_legacy_bin(&file, legacy_dir, target)? {
            files.push(file);
        }
    }
    Ok(files)
}

fn is_legacy_bin(bin_path: &Path, legacy_dir: &Path, target: Option<&Path>) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(bin_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let Some(bin_dir) = bin_path.parent() else { return Ok(false) };
    if metadata.is_symlink() {
        return fs::read_link(bin_path)
            .map(|target| points_into(&bin_dir.join(target), legacy_dir));
    }
    if !metadata.is_file() {
        return Ok(false);
    }
    if let Some(target) = target
        && is_legacy_hard_link(bin_path, target, legacy_dir)?
    {
        return Ok(true);
    }
    if metadata.len() > MAX_SHIM_BYTES {
        return Ok(false);
    }
    match fs::read_to_string(bin_path) {
        Ok(content) => Ok(shim_targets_dir(&content, bin_dir, legacy_dir)),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_legacy_hard_link(bin_path: &Path, target: &Path, legacy_dir: &Path) -> io::Result<bool> {
    let check = || {
        let target = fs::canonicalize(target)?;
        let legacy_dir = fs::canonicalize(legacy_dir)?;
        if !is_subdir(&legacy_dir, &target) {
            return Ok(false);
        }
        same_file::is_same_file(bin_path, target)
    };
    match check() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        result => result,
    }
}

/// Whether `path` lies under `dir`, spelled as given or through the
/// symlinks of either: macOS spells a temp dir as `/var` and `/private/var`.
fn points_into(path: &Path, dir: &Path) -> bool {
    if is_subdir(dir, &lexical_normalize(path)) {
        return true;
    }
    match (realpath_missing(dir), realpath_missing(path)) {
        (Ok(dir), Ok(path)) => is_subdir(&dir, &path),
        _ => false,
    }
}

/// Whether a shim in `shim_dir` runs something under `dir`, which the shim
/// names either by its absolute path, in either spelling of a symlinked
/// directory, or relative to its own directory, with either kind of slash.
fn shim_targets_dir(shim_content: &str, shim_dir: &Path, dir: &Path) -> bool {
    let content = shim_content.replace('\\', "/");
    let spelled = |path: &Path| path.to_string_lossy().replace('\\', "/");
    let mut prefixes = vec![spelled(dir)];
    if let Ok(real_dir) = realpath_missing(dir) {
        prefixes.push(spelled(&real_dir));
    }
    if let Ok(relative) = dir.strip_prefix(shim_dir) {
        prefixes.push(spelled(relative));
    }
    prefixes
        .iter()
        .any(|prefix| content.contains(&format!("{prefix}/node_modules/")))
}

/// Reinstall the packages of the previous global layout as groups of the
/// current one, then delete the previous project together with the bins it
/// linked into the pnpm home. A package a current group already declares
/// is left alone: reinstalling it would replace that group, the packages
/// installed together with it included. The previous project stays for
/// the next `update -g` while a package failed to install or while such a
/// declared package is still missing its files.
pub async fn migrate_legacy_global_packages<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    let Some(legacy) = LegacyGlobalLayout::find(&global_pkg_dir)? else {
        return Ok(());
    };
    check_bin_dir(&global_bin_dir)?;
    fs::create_dir_all(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global packages directory")?;
    clean_orphaned_install_dirs(&global_pkg_dir);
    let current = CurrentGroups::scan(&global_pkg_dir)?;
    let selectors = legacy.selectors_to_migrate(&current.declared);
    let target = GlobalInstallTarget {
        base_config,
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
    };
    let every_package_migrated = target.migrate_legacy_groups::<Reporter>(
        &legacy,
        &selectors,
        range_spec_style,
        supported_architectures,
    )
    .await;
    let awaiting_restore = legacy.aliases_awaiting_restore(&current);
    if every_package_migrated && awaiting_restore.is_empty() {
        return legacy.remove::<Reporter>();
    }
    warn_legacy_kept::<Reporter>(&legacy.dir, every_package_migrated, &awaiting_restore);
    Ok(())
}

fn warn_legacy_kept<Reporter: self::Reporter>(
    legacy_dir: &Path,
    every_package_migrated: bool,
    awaiting_restore: &[&str],
) {
    let dir = legacy_dir.display();
    if !every_package_migrated {
        warn_global::<Reporter>(&format!(
            "Kept {dir} for the packages that failed to migrate. \
             The next \"pnpm update -g\" retries them; delete the directory to skip them.",
        ));
        return;
    }
    warn_global::<Reporter>(&format!(
        "Kept {dir} until {} are installed again: a current global package declares them \
         without their files. \"pnpm update -g\" restores a global package that lost all of \
         its files; otherwise remove it with \"pnpm remove -g\". The next \"pnpm update -g\" \
         then removes the directory.",
        awaiting_restore.join(", "),
    ));
}

impl GlobalInstallTarget<'_> {
    /// Install each selector as its own group, closing none of them with a
    /// summary: `update -g` closes the run with its own. Returns whether
    /// every one of them installed.
    async fn migrate_legacy_groups<Reporter: self::Reporter + 'static>(
        &self,
        legacy: &LegacyGlobalLayout,
        selectors: &[MigrationSelector],
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> bool {
        if selectors.is_empty() {
            return true;
        }
        let dir = legacy.dir.display();
        let aliases: Vec<&str> = selectors
            .iter()
            .map(|to_migrate| to_migrate.alias.as_str())
            .collect();
        info_global::<Reporter>(&format!(
            "Migrating global packages from {dir}: {}",
            aliases.join(", "),
        ));
        let mut every_package_migrated = true;
        for MigrationSelector { alias, selector } in selectors {
            let filter = EventFilter::GlobalUpdateMaterialization.apply();
            let result = self.add_group::<Reporter>(
                std::slice::from_ref(selector),
                range_spec_style,
                supported_architectures.clone(),
                &[],
            )
            .await;
            drop(filter);
            if let Err(error) = result {
                every_package_migrated = false;
                warn_migration_failure::<Reporter>(alias, &legacy.dir, &error);
            }
        }
        every_package_migrated
    }
}

fn warn_migration_failure<Reporter: self::Reporter>(
    alias: &str,
    legacy_dir: &Path,
    error: &miette::Report,
) {
    let cause = error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ");
    let dir = legacy_dir.display();
    warn_global::<Reporter>(&format!("Failed to migrate {alias} from {dir}: {cause}"));
}

fn info_global<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: message.to_string(),
    }));
}

#[cfg(test)]
mod tests;
