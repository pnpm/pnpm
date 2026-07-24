use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::{Identifier, Version};
use pacquet_config::Config;
use pacquet_executor::{RunPostinstallHooks, run_lifecycle_hook};
use pacquet_package_manifest::PackageManifest;
use pacquet_publish::{Host, RunCommand, is_git_repo, is_working_tree_clean};
use pacquet_versioning::{
    AssembleReleasePlanOptions, apply_release_plan, assemble_release_plan, read_change_intents,
    read_ledger,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::cli_args::{
    change::{render_release_plan, to_engine_projects},
    changelog::{confirmed_published_versions, unpublished_release_dirs},
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
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

    /// Print the release plan the pending change intents produce without
    /// applying it.
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
    #[clap(long)]
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

    /// Output release details in JSON format.
    #[clap(long = "json")]
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
    pub async fn run<Reporter: pacquet_reporter::Reporter>(
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
    fn npm_style_bump<Reporter: pacquet_reporter::Reporter>(
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
        if config.git_checks
            && !self.no_git_checks
            && is_git_repo::<Host>(&git_cwd)
            && !is_working_tree_clean::<Host>(&git_cwd)
        {
            return Err(VersionError::UncleanWorkingTree.into());
        }

        let mut changes: Vec<VersionChange> = Vec::new();
        if recursive {
            let base = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
            let (projects, _) = discover_workspace_projects(&base)?;
            let selection =
                select_recursive_projects(&projects, config, &base, AutoExcludeRoot::Disabled)?;
            for pkg_dir in selection.selected.keys() {
                if let Some(change) =
                    self.bump_package_version::<Reporter>(pkg_dir, &bump, config, dir)?
                {
                    changes.push(change);
                }
            }
        } else if let Some(change) =
            self.bump_package_version::<Reporter>(dir, &bump, config, dir)?
        {
            changes.push(change);
        }

        if changes.is_empty() {
            return Err(VersionError::NoPackagesToVersion.into());
        }

        // In recursive mode, multiple packages can be bumped to different
        // versions in a single run, and there is no obvious single version to
        // tag the commit with. Skip the git commit and tag entirely then.
        if !recursive && !self.no_git_tag_version && is_git_repo::<Host>(&git_cwd) {
            self.commit_and_tag(&changes[0], &git_cwd)?;
        }

        for change in &changes {
            run_version_lifecycle_hook::<Reporter>("postversion", change, config, dir)?;
        }

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
            return Ok(());
        }

        use std::fmt::Write as _;
        let mut output = String::from("Version bumped successfully:\n");
        for change in &changes {
            writeln!(
                output,
                "{}: {} → {}",
                change.name, change.current_version, change.new_version,
            )
            .expect("write to string");
        }
        print!("{output}");
        Ok(())
    }

    /// Bump one package's manifest, running its `preversion` and `version`
    /// lifecycle hooks around the write. Returns `None` — bumping nothing —
    /// when the manifest has no name or no version.
    fn bump_package_version<Reporter: pacquet_reporter::Reporter>(
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

        let Ok(current_version) = Version::parse(&current) else {
            return Err(VersionError::InvalidVersion {
                dir: pkg_dir.display().to_string(),
                version: current,
            }
            .into());
        };

        let pre_change = VersionChange {
            name: name.clone(),
            current_version: current.clone(),
            new_version: current.clone(),
            path: pkg_dir.to_path_buf(),
            manifest_path: manifest_path.clone(),
        };
        run_version_lifecycle_hook::<Reporter>("preversion", &pre_change, config, init_cwd)?;

        let new_version = match bump {
            Bump::Explicit(version) => version.clone(),
            // An empty --preid means "no preid", as in the TypeScript CLI,
            // where the empty string is falsy to semver's inc().
            Bump::Release(release) => inc(
                &current_version,
                *release,
                self.preid.as_deref().filter(|preid| !preid.is_empty()),
            ),
        }
        .to_string();

        if new_version == current && !self.allow_same_version {
            return Err(VersionError::VersionNotChanged { version: current }.into());
        }

        manifest
            .value_mut()
            .as_object_mut()
            .expect("package.json is an object — its version field was just read")
            .insert("version".to_string(), Value::String(new_version.clone()));
        manifest.save().wrap_err_with(|| format!("saving {}", manifest_path.display()))?;

        let change = VersionChange {
            name,
            current_version: current,
            new_version,
            path: pkg_dir.to_path_buf(),
            manifest_path,
        };
        run_version_lifecycle_hook::<Reporter>("version", &change, config, init_cwd)?;
        Ok(Some(change))
    }

    /// Stage the bumped manifest and record the bump as a commit plus an
    /// annotated (or signed) tag, mirroring the TypeScript `commitAndTag`.
    fn commit_and_tag(&self, change: &VersionChange, cwd: &Path) -> miette::Result<()> {
        let message = self.message.as_deref().unwrap_or("%s").replace("%s", &change.new_version);
        let tag_name = format!("{}{}", self.tag_version_prefix, change.new_version);

        let Ok(relative) = change.manifest_path.strip_prefix(cwd) else {
            return Err(VersionError::InvalidManifestPath {
                path: change.manifest_path.display().to_string(),
            }
            .into());
        };
        let manifest_rel: String = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        run_git(cwd, &["add", &manifest_rel])?;

        let mut commit_args = vec!["commit", "-m", &message];
        if self.no_commit_hooks {
            commit_args.push("--no-verify");
        }
        // The manifest write can leave nothing staged on an
        // --allow-same-version run. Pass --allow-empty in that case to let
        // the tag point at the current HEAD as a deliberate marker.
        if self.allow_same_version {
            commit_args.push("--allow-empty");
        }
        run_git(cwd, &commit_args)?;

        let mut tag_args = vec!["tag", if self.sign_git_tag { "-s" } else { "-a" }];
        tag_args.extend([tag_name.as_str(), "-m", &message]);
        run_git(cwd, &tag_args)
    }

    async fn release_from_intents(&self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(VersionError::ReleaseOutsideWorkspace.into());
        };

        if !self.dry_run
            && config.git_checks
            && !self.no_git_checks
            && is_git_repo::<Host>(&workspace_dir)
            && !is_working_tree_clean::<Host>(&workspace_dir)
        {
            return Err(VersionError::UncleanWorkingTree.into());
        }

        let intents = read_change_intents(&workspace_dir)?;
        let ledger = read_ledger(&workspace_dir)?;
        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);

        let filter = if config.filter.is_empty() {
            None
        } else {
            Some(
                selected_projects(&projects, config, &workspace_dir)?
                    .into_iter()
                    .map(|(_, dir)| dir)
                    .collect::<HashSet<String>>(),
            )
        };
        let is_filtered = filter.is_some();
        let assemble = |unpublished_dirs: HashSet<String>| {
            assemble_release_plan(
                &engine_projects,
                &workspace_dir,
                &intents,
                &ledger,
                Some(&config.versioning),
                &AssembleReleasePlanOptions {
                    filter: filter.clone(),
                    snapshot_suffix: None,
                    enforce_workspace_protocol: true,
                    unpublished_dirs,
                },
            )
        };
        let unpublished_dirs = unpublished_release_dirs(config, &assemble(HashSet::new())?).await?;
        let plan = assemble(unpublished_dirs)?;

        if plan.releases.is_empty() {
            // A full (unfiltered) run garbage-collects the intent files an
            // empty plan leaves behind: declined ("none"-only) intents and
            // files a merge resurrected after every named package had already
            // consumed them. A filtered run must not — "nothing pending in
            // this scope" is no reason to delete prose belonging to packages
            // outside the filter.
            if !self.dry_run && !is_filtered {
                let confirmed = confirmed_published_versions(config, &workspace_dir).await?;
                apply_release_plan(
                    &plan,
                    &workspace_dir,
                    &engine_projects,
                    &intents,
                    Some(&config.versioning),
                    &confirmed,
                )?;
            }
            if self.json {
                println!("[]");
            } else {
                println!(r#"No pending changes. Record one with "pnpm change"."#);
            }
            return Ok(());
        }
        if self.dry_run {
            println!("{}", render_release_plan(&plan));
            return Ok(());
        }

        let confirmed = confirmed_published_versions(config, &workspace_dir).await?;
        let applied = apply_release_plan(
            &plan,
            &workspace_dir,
            &engine_projects,
            &intents,
            Some(&config.versioning),
            &confirmed,
        )?;

        if self.json {
            println!("{}", serde_json::to_string_pretty(&applied).into_diagnostic()?);
            return Ok(());
        }

        use std::fmt::Write as _;
        let mut output = String::from("Versions applied:\n");
        for release in &applied {
            writeln!(
                output,
                "{}: {} → {}",
                release.name, release.current_version, release.new_version,
            )
            .expect("write to string");
        }
        println!("{output}");
        Ok(())
    }
}

/// Run one `preversion` / `version` / `postversion` script of the bumped
/// package, when the manifest declares it and scripts are not ignored.
/// The manifest is re-read so the `version` and `postversion` hooks see
/// the bumped version.
fn run_version_lifecycle_hook<Reporter: pacquet_reporter::Reporter>(
    stage: &str,
    change: &VersionChange,
    config: &Config,
    init_cwd: &Path,
) -> miette::Result<()> {
    if config.ignore_scripts {
        return Ok(());
    }
    let manifest = PackageManifest::from_path(change.manifest_path.clone())
        .wrap_err_with(|| format!("reading {}", change.manifest_path.display()))?;
    let Some(script) = manifest
        .value()
        .get("scripts")
        .and_then(|scripts| scripts.get(stage))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty())
        .map(ToString::to_string)
    else {
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
        node_gyp_bin: None,
        scripts_prepend_node_path: super::run::exec_scripts_prepend_node_path(
            config.scripts_prepend_node_path,
        ),
        script_shell: script_shell.as_deref(),
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

/// A parsed version argument: an exact version to set, or a release type to
/// increment by.
#[derive(Debug)]
enum Bump {
    Explicit(Version),
    Release(ReleaseType),
}

#[derive(Debug, Clone, Copy)]
enum ReleaseType {
    Major,
    Minor,
    Patch,
    Premajor,
    Preminor,
    Prepatch,
    Prerelease,
}

/// Parse the version argument: a valid semver version wins (like upstream's
/// `semver.valid`, so a leading `v` is accepted and stripped), then the
/// release-type keywords.
fn parse_bump(raw: &str) -> Result<Bump, VersionError> {
    if let Ok(version) = Version::parse(raw) {
        return Ok(Bump::Explicit(version));
    }
    let release = match raw {
        "major" => ReleaseType::Major,
        "minor" => ReleaseType::Minor,
        "patch" => ReleaseType::Patch,
        "premajor" => ReleaseType::Premajor,
        "preminor" => ReleaseType::Preminor,
        "prepatch" => ReleaseType::Prepatch,
        "prerelease" => ReleaseType::Prerelease,
        _ => return Err(VersionError::InvalidBump { raw: raw.to_string() }),
    };
    Ok(Bump::Release(release))
}

/// Increment `version` by `release`, following node-semver's `inc()` with its
/// default identifier base of `0`: bumping from a prerelease of the next
/// major/minor/patch merely finalizes it, the `pre*` types start a `.0`
/// prerelease (prefixed with `preid` when given), and `prerelease` increments
/// the right-most numeric identifier.
fn inc(version: &Version, release: ReleaseType, preid: Option<&str>) -> Version {
    let mut next = version.clone();
    next.build = Vec::new();
    match release {
        ReleaseType::Major => {
            if next.pre_release.is_empty() || next.minor != 0 || next.patch != 0 {
                next.major += 1;
            }
            next.minor = 0;
            next.patch = 0;
            next.pre_release = Vec::new();
        }
        ReleaseType::Minor => {
            if next.pre_release.is_empty() || next.patch != 0 {
                next.minor += 1;
            }
            next.patch = 0;
            next.pre_release = Vec::new();
        }
        ReleaseType::Patch => {
            if next.pre_release.is_empty() {
                next.patch += 1;
            }
            next.pre_release = Vec::new();
        }
        ReleaseType::Premajor => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Preminor => {
            next.minor += 1;
            next.patch = 0;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Prepatch => {
            next.patch += 1;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Prerelease => {
            if next.pre_release.is_empty() {
                next.patch += 1;
                next.pre_release = initial_prerelease(preid);
            } else {
                increment_prerelease(&mut next.pre_release, preid);
            }
        }
    }
    next
}

/// The prerelease identifiers a fresh `pre*` bump starts with: `preid.0`, or
/// a bare `0` without a preid.
fn initial_prerelease(preid: Option<&str>) -> Vec<Identifier> {
    match preid {
        Some(preid) => vec![make_identifier(preid), Identifier::Numeric(0)],
        None => vec![Identifier::Numeric(0)],
    }
}

/// Increment a non-empty prerelease in place: bump the right-most numeric
/// identifier (appending `.0` when there is none), then — when a preid is
/// given — keep the result only if it is already `preid.<number>`, otherwise
/// restart at `preid.0`.
fn increment_prerelease(pre_release: &mut Vec<Identifier>, preid: Option<&str>) {
    let mut bumped = false;
    for identifier in pre_release.iter_mut().rev() {
        if let Identifier::Numeric(number) = identifier {
            *number += 1;
            bumped = true;
            break;
        }
    }
    if !bumped {
        pre_release.push(Identifier::Numeric(0));
    }

    if let Some(preid) = preid {
        let first_matches =
            pre_release.first().is_some_and(|first| identifier_text(first) == preid);
        let second_is_numeric = matches!(pre_release.get(1), Some(Identifier::Numeric(_)));
        if !(first_matches && second_is_numeric) {
            *pre_release = vec![make_identifier(preid), Identifier::Numeric(0)];
        }
    }
}

fn make_identifier(text: &str) -> Identifier {
    match text.parse::<u64>() {
        Ok(number) => Identifier::Numeric(number),
        Err(_) => Identifier::AlphaNumeric(text.to_string()),
    }
}

fn identifier_text(identifier: &Identifier) -> String {
    match identifier {
        Identifier::Numeric(number) => number.to_string(),
        Identifier::AlphaNumeric(text) => text.clone(),
    }
}

/// Build the canonical cross-stack error for an invalid version from Git.
fn invalid_version_from_git(
    cwd: &Path,
    tag_version_prefix: &str,
    reason: impl Into<String>,
) -> VersionError {
    VersionError::InvalidVersionFromGit {
        dir: cwd.display().to_string(),
        tag_version_prefix: tag_version_prefix.to_string(),
        reason: reason.into(),
    }
}

fn version_from_git(cwd: &Path, tag_version_prefix: &str) -> Result<Version, VersionError> {
    let pattern = format!("{tag_version_prefix}*.*.*");
    let args = ["describe", "--tags", "--abbrev=0", "--always", "--match", pattern.as_str()];
    let output = <Host as RunCommand>::run("git", &args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: args.join(" "), stderr: err.to_string() }
    })?;

    if !output.success {
        return Err(VersionError::GitCommandFailed {
            args: args.join(" "),
            stderr: output.stderr.trim().to_string(),
        });
    }

    let tag = output.stdout.trim();
    let tag_args = ["tag", "--list", "--", tag];
    let matching_tag = <Host as RunCommand>::run("git", &tag_args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: tag_args.join(" "), stderr: err.to_string() }
    })?;

    if !matching_tag.success {
        return Err(VersionError::GitCommandFailed {
            args: tag_args.join(" "),
            stderr: matching_tag.stderr.trim().to_string(),
        });
    }

    if matching_tag.stdout.trim() != tag {
        return Err(invalid_version_from_git(cwd, tag_version_prefix, "no matching Git tag found"));
    }

    let Some(raw_version) = tag.strip_prefix(tag_version_prefix) else {
        return Err(invalid_version_from_git(
            cwd,
            tag_version_prefix,
            format!("tag is not a valid version: {tag:?}"),
        ));
    };

    Version::parse(raw_version).map_err(|_| {
        invalid_version_from_git(
            cwd,
            tag_version_prefix,
            format!("tag is not a valid version: {tag:?}"),
        )
    })
}

/// Run a git command in `cwd`, failing with the command line and git's stderr
/// when it exits non-zero.
fn run_git(cwd: &Path, args: &[&str]) -> miette::Result<()> {
    let output = <Host as RunCommand>::run("git", args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: args.join(" "), stderr: err.to_string() }
    })?;
    if !output.success {
        return Err(VersionError::GitCommandFailed {
            args: args.join(" "),
            stderr: output.stderr.trim().to_string(),
        }
        .into());
    }
    Ok(())
}

/// The projects the active `--filter` selectors pick, in graph order, as
/// `(name, workspace-relative dir)` pairs.
pub(crate) fn selected_projects(
    projects: &[pacquet_workspace::Project],
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<Vec<(Option<String>, String)>> {
    let selection =
        select_recursive_projects(projects, config, workspace_dir, AutoExcludeRoot::Disabled)?;
    Ok(selection
        .selected
        .iter()
        .map(|(root_dir, node)| {
            let name = node
                .package
                .project
                .manifest
                .value()
                .get("name")
                .and_then(|name| name.as_str())
                .map(ToString::to_string);
            (name, pacquet_versioning::to_project_dir(workspace_dir, root_dir))
        })
        .collect())
}

#[cfg(test)]
mod tests;
