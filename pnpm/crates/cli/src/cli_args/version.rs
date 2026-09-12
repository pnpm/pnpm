pub(crate) use release::selected_projects;

use crate::cli_args::{
    change::{render_release_plan, to_engine_projects},
    changelog::{self, confirmed_published_versions, unpublished_release_dirs},
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};
use bump::{Bump, inc, parse_bump, parse_current_version};
use clap::Args;
use derive_more::{Display, Error};
use git::version_from_git;
use miette::{Context, Diagnostic};
use node_semver::{Identifier, Version};
use pnpm_config::Config;
use pnpm_executor::{RunPostinstallHooks, run_lifecycle_hook};
use pnpm_package_manifest::PackageManifest;
use pnpm_publish::{Host, RunCommand, is_git_repo, is_working_tree_clean};
use pnpm_versioning::{
    AssembleReleasePlanOptions, apply_release_plan, assemble_release_plan, read_change_intents,
    read_ledger,
};

use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Bump the version of a package: `pnpm version <bump|semver>` applies an
/// npm-style bump to the current package (or, with `-r`, to every selected
/// workspace package), while the bare `pnpm version -r` applies the pending
/// change intents.
#[derive(Debug, Args)]
pub struct VersionArgs {
    /// A valid semver version (e.g. 1.2.3) or one of: major, minor, patch,
    /// premajor, preminor, prepatch, prerelease, from-git. Omit it and pass `-r` to
    /// apply the pending change intents instead.
    pub params: Vec<String>,

    /// Print what the command would do without changing anything.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Don't check if the working tree is clean.
    #[clap(long = "no-git-checks")]
    pub no_git_checks: bool,

    /// Sets the prerelease identifier (e.g. alpha, beta, rc).
    #[clap(long)]
    pub preid: Option<String>,

    /// Allow bumping to the same version.
    #[clap(long = "allow-same-version")]
    pub allow_same_version: bool,

    /// Commit message. "%s" is replaced with the new version. Default is "%s".
    #[clap(long, short = 'm')]
    pub message: Option<String>,

    /// Don't create a commit or tag for the version bump. Git commits and
    /// tags are always skipped in recursive mode.
    #[clap(long = "no-git-tag-version")]
    pub no_git_tag_version: bool,

    /// Skip running git commit hooks when committing the version bump.
    #[clap(long = "no-commit-hooks")]
    pub no_commit_hooks: bool,

    /// Sign the generated git tag with GPG.
    #[clap(long = "sign-git-tag")]
    pub sign_git_tag: bool,

    /// Sets the tag prefix. Default is "v". Set to empty string to remove
    /// the prefix.
    #[clap(long = "tag-version-prefix", default_value = "v")]
    pub tag_version_prefix: String,

    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,
}

/// Errors of `pnpm version`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
enum VersionError {
    #[display(
        "A version argument is required. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    MissingBump,

    #[display(
        "Invalid version argument: {raw}. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    InvalidBump { raw: String },

    #[display(
        "Could not determine a valid version from Git in {dir:?} using tag prefix {tag_version_prefix:?}: {reason}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_FROM_GIT))]
    InvalidVersionFromGit { dir: String, tag_version_prefix: String, reason: String },

    #[display("Invalid version in {dir}: {version}")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION))]
    InvalidVersion { dir: String, version: String },

    #[display("Version was not changed: {version}")]
    #[diagnostic(code(ERR_PNPM_VERSION_NOT_CHANGED))]
    VersionNotChanged { version: String },

    #[display("No packages to version")]
    #[diagnostic(code(ERR_PNPM_NO_PACKAGES_TO_VERSION))]
    NoPackagesToVersion,

    #[display("Cannot stage manifest outside of git cwd: {path}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MANIFEST_PATH))]
    InvalidManifestPath { path: String },

    #[display("git {args} failed: {stderr}")]
    #[diagnostic(code(ERR_PNPM_GIT_COMMAND_FAILED))]
    GitCommandFailed { args: String, stderr: String },

    #[display(
        r#"The bare "pnpm version -r" form consumes change intents and is only supported in a workspace"#
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    ReleaseOutsideWorkspace,

    #[display("Working tree is not clean. Commit or stash your changes.")]
    #[diagnostic(code(ERR_PNPM_UNCLEAN_WORKING_TREE))]
    UncleanWorkingTree,
}

impl VersionArgs {
    pub async fn run<Reporter: pnpm_reporter::Reporter>(
        self,
        config: &Config,
        dir: &Path,
        recursive: bool,
    ) -> miette::Result<()> {
        match self.params.first().map(String::as_str) {
            None if recursive => self.release_from_intents(config).await,
            None => Err(VersionError::MissingBump.into()),
            Some(_) => self.npm_style_bump::<Reporter>(config, dir, recursive),
        }
    }

    /// Apply an npm-style bump — `pnpm version <major|minor|…|x.y.z>` — to
    /// the package at `dir`, or to every selected workspace package when
    /// `recursive`. Mirrors the TypeScript handler: git-tree check, per-
    /// package bump with `preversion`/`version` hooks, a commit and tag for
    /// the single-package form, then `postversion` hooks and the report.
    fn npm_style_bump<Reporter: pnpm_reporter::Reporter>(
        &self,
        config: &Config,
        dir: &Path,
        recursive: bool,
    ) -> miette::Result<()> {
        let raw = self.params[0].as_str();
        let git_cwd = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let bump = if raw == "from-git" {
            Bump::Explicit(version_from_git(&git_cwd, &self.tag_version_prefix)?)
        } else {
            parse_bump(raw)?
        };
        if !self.dry_run
            && config.git_checks
            && !self.no_git_checks
            && is_git_repo::<Host>(&git_cwd)
            && !is_working_tree_clean::<Host>(&git_cwd)
        {
            return Err(VersionError::UncleanWorkingTree.into());
        }

        let changes = self.collect_version_changes::<Reporter>(&bump, config, dir, recursive)?;
        if changes.is_empty() {
            return Err(VersionError::NoPackagesToVersion.into());
        }

        // In recursive mode, multiple packages can be bumped to different
        // versions in a single run, and there is no obvious single version to
        // tag the commit with. Skip the git commit and tag entirely then.
        if !self.dry_run && !recursive && !self.no_git_tag_version && is_git_repo::<Host>(&git_cwd)
        {
            self.commit_and_tag(&changes[0], &git_cwd)?;
        }

        for change in &changes {
            run_version_lifecycle_hook::<Reporter>(
                "postversion",
                change,
                config,
                dir,
                self.dry_run,
            )?;
        }

        self.report_version_changes(&changes);
        Ok(())
    }

    /// Bump every package this run covers: the selected workspace projects
    /// when `recursive`, otherwise the package at `dir` alone.
    fn collect_version_changes<Reporter: pnpm_reporter::Reporter>(
        &self,
        bump: &Bump,
        config: &Config,
        dir: &Path,
        recursive: bool,
    ) -> miette::Result<Vec<VersionChange>> {
        if !recursive {
            let change = self.bump_package_version::<Reporter>(dir, bump, config, dir)?;
            return Ok(change.into_iter().collect());
        }
        let base = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let (projects, _) = discover_workspace_projects(&base, config)?;
        let selection =
            select_recursive_projects(&projects, config, &base, AutoExcludeRoot::Disabled)?;
        let mut changes = Vec::new();
        for pkg_dir in selection.selected.keys() {
            if let Some(change) =
                self.bump_package_version::<Reporter>(pkg_dir, bump, config, dir)?
            {
                changes.push(change);
            }
        }
        Ok(changes)
    }

    fn report_version_changes(&self, changes: &[VersionChange]) {
        if self.json {
            let entries: Vec<Value> = changes
                .iter()
                .map(|change| {
                    json!({
                        "name": change.name,
                        "currentVersion": change.current_version,
                        "newVersion": change.new_version,
                        "path": change.path,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&entries).expect("serialize changes"));
            return;
        }

        use std::fmt::Write as _;
        let mut output = String::from(if self.dry_run {
            "Version bump plan:\n"
        } else {
            "Version bumped successfully:\n"
        });
        for change in changes {
            writeln!(
                output,
                "{}: {} → {}",
                change.name, change.current_version, change.new_version,
            )
            .expect("write to string");
        }
        print!("{output}");
    }

    /// Bump one package's manifest, running its `preversion` and `version`
    /// lifecycle hooks around the write. Both the write and the hooks are
    /// skipped on a dry run. Returns `None` — bumping nothing — when the
    /// manifest has no name or no version.
    fn bump_package_version<Reporter: pnpm_reporter::Reporter>(
        &self,
        pkg_dir: &Path,
        bump: &Bump,
        config: &Config,
        init_cwd: &Path,
    ) -> miette::Result<Option<VersionChange>> {
        let manifest_path = pkg_dir.join("package.json");
        let mut manifest = PackageManifest::from_path(manifest_path.clone())
            .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;

        let name = manifest.value().get("name").and_then(Value::as_str).unwrap_or_default();
        let current = manifest.value().get("version").and_then(Value::as_str).unwrap_or_default();
        if name.is_empty() || current.is_empty() {
            return Ok(None);
        }
        let (name, current) = (name.to_string(), current.to_string());

        let current_version = parse_current_version(pkg_dir, &current)?;

        self.preversion_hook::<Reporter>(
            pkg_dir,
            &manifest_path,
            &name,
            &current,
            config,
            init_cwd,
        )?;

        let new_version = self.next_package_version(&current_version, bump, &current)?;

        manifest
            .value_mut()
            .as_object_mut()
            .expect("package.json is an object — its version field was just read")
            .insert("version".to_string(), Value::String(new_version.clone()));
        if !self.dry_run {
            manifest.save().wrap_err_with(|| format!("saving {}", manifest_path.display()))?;
        }

        let change = VersionChange {
            name,
            current_version: current,
            new_version,
            path: pkg_dir.to_path_buf(),
            manifest_path,
        };
        run_version_lifecycle_hook::<Reporter>("version", &change, config, init_cwd, self.dry_run)?;
        Ok(Some(change))
    }

    fn preversion_hook<Reporter: pnpm_reporter::Reporter>(
        &self,
        pkg_dir: &Path,
        manifest_path: &Path,
        name: &str,
        current: &str,
        config: &Config,
        init_cwd: &Path,
    ) -> miette::Result<()> {
        let pre_change = VersionChange {
            name: name.to_string(),
            current_version: current.to_string(),
            new_version: current.to_string(),
            path: pkg_dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
        };
        run_version_lifecycle_hook::<Reporter>(
            "preversion",
            &pre_change,
            config,
            init_cwd,
            self.dry_run,
        )
    }

    fn next_package_version(
        &self,
        current_version: &Version,
        bump: &Bump,
        current: &str,
    ) -> miette::Result<String> {
        let new_version = match bump {
            Bump::Explicit(version) => version.clone(),
            // An empty --preid means "no preid", as in the TypeScript CLI,
            // where the empty string is falsy to semver's inc().
            Bump::Release(release) => inc(
                current_version,
                *release,
                self.preid.as_deref().filter(|preid| !preid.is_empty()),
            ),
        }
        .to_string();

        if new_version == current && !self.allow_same_version {
            return Err(VersionError::VersionNotChanged { version: current.to_string() }.into());
        }

        Ok(new_version)
    }
}

/// Run one `preversion` / `version` / `postversion` script of the bumped
/// package, when the manifest declares it, scripts are not ignored and this
/// is not a dry run. The manifest is re-read so the `version` and
/// `postversion` hooks see the bumped version.
fn run_version_lifecycle_hook<Reporter: pnpm_reporter::Reporter>(
    stage: &str,
    change: &VersionChange,
    config: &Config,
    init_cwd: &Path,
    dry_run: bool,
) -> miette::Result<()> {
    if config.ignore_scripts || dry_run {
        return Ok(());
    }
    let manifest = PackageManifest::from_path(change.manifest_path.clone())
        .wrap_err_with(|| format!("reading {}", change.manifest_path.display()))?;
    let declared = manifest
        .value()
        .get("scripts")
        .and_then(|scripts| scripts.get(stage))
        .and_then(Value::as_str);
    let Some(script) = declared.filter(|script| !script.is_empty()).map(ToString::to_string) else {
        return Ok(());
    };

    let root_modules_dir = change.path.join(&config.modules_dir);
    let script_shell = config.script_shell.as_ref().map(PathBuf::from);
    let run_opts = RunPostinstallHooks {
        dep_path: &change.name,
        pkg_root: &change.path,
        root_modules_dir: &root_modules_dir,
        init_cwd,
        extra_bin_paths: &config.extra_bin_paths,
        extra_env: &config.extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(&config.user_agent),
        unsafe_perm: config.unsafe_perm,
        node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
        scripts_prepend_node_path: super::run::exec_scripts_prepend_node_path(
            config.scripts_prepend_node_path,
        ),
        script_shell: script_shell.as_deref(),
        shell_emulator: config.shell_emulator,
        optional: false,
    };
    let parent_env: HashMap<String, String> = std::env::vars().collect();
    run_lifecycle_hook::<Reporter>(stage, &script, &run_opts, manifest.value(), &parent_env)
        .map_err(miette::Report::new)
}

/// One package's version bump: what it was, what it became, and where its
/// manifest lives.
#[derive(Debug)]
struct VersionChange {
    name: String,
    current_version: String,
    new_version: String,
    path: PathBuf,
    manifest_path: PathBuf,
}

#[cfg(test)]
mod tests;

mod bump;

mod release;

mod git;
