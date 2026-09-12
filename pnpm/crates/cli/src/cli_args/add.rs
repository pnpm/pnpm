pub(crate) use execution::{add_package, add_packages};

use crate::{
    State,
    cargo_manifest::CargoDependencyKind,
    cli_args::{
        install::resolve_bool_override, lockfile_dir::LockfileDirArg,
        pipelines::InstallFamilySelection, recursive,
        supported_architectures::SupportedArchitecturesArgs, workspace_option::workspace_link_root,
    },
    config_deps,
    engine_pm::{
        error::EngineError,
        pin::{
            declared_package_manager, describe_pin, record_package_manager_pin, resolve_project_pin,
        },
        selector::tool_install_selector,
    },
};
use clap::Args;
use derive_more::{Display, Error};

use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_package_manager::{Add, build_workspace_packages_map, parse_allow_build_selector};
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::WorkspacePackages;
use pnpm_workspace_manifest_writer::set_allow_builds;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Args)]
pub struct AddDependencyOptions {
    /// Install the specified packages as regular dependencies.
    #[clap(short = 'P', long)]
    save_prod: bool,
    /// Install the specified packages as devDependencies.
    #[clap(short = 'D', long)]
    save_dev: bool,
    /// Install the specified packages as optionalDependencies.
    #[clap(short = 'O', long)]
    save_optional: bool,
    /// Install crate: packages as Cargo build dependencies.
    #[clap(long = "save-build")]
    save_build: bool,
    /// Using --save-peer will add one or more packages to peerDependencies and install them as dev dependencies
    #[clap(long, overrides_with = "no_save_peer")]
    save_peer: bool,
    /// Don't add the packages to peerDependencies, overriding a
    /// `savePeer: true` setting.
    #[clap(long = "no-save-peer", overrides_with = "save_peer")]
    no_save_peer: bool,
}

impl AddDependencyOptions {
    pub(crate) fn python_development(&self) -> miette::Result<bool> {
        if self.save_build || self.save_optional || self.save_peer {
            return Err(miette::miette!(
                "pypi: dependencies do not support --save-build, --save-optional or --save-peer"
            ));
        }
        if self.save_prod && self.save_dev {
            return Err(miette::miette!(
                "pypi: dependencies do not support combining --save-prod and --save-dev"
            ));
        }
        Ok(self.save_dev)
    }

    pub(crate) fn save_build(&self) -> bool {
        self.save_build
    }

    /// `--save-peer` / `--no-save-peer` layered over the `savePeer` setting.
    fn with_save_peer_setting(self, save_peer: bool) -> Self {
        Self {
            save_peer: resolve_bool_override(self.save_peer, self.no_save_peer, save_peer),
            ..self
        }
    }

    /// Whether to add entry to `"dependencies"`.
    fn save_prod(&self) -> bool {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_build,
            save_peer,
            no_save_peer: _,
        } = self;
        save_prod || (!save_dev && !save_optional && !save_build && !save_peer)
    }

    /// Whether to add entry to `"devDependencies"`.
    fn save_dev(&self) -> bool {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_build,
            save_peer,
            no_save_peer: _,
        } = self;
        save_dev || (!save_prod && !save_optional && !save_build && save_peer)
    }

    /// Whether to add entry to `"optionalDependencies"`.
    fn save_optional(&self) -> bool {
        self.save_optional
    }

    /// Whether to add entry to `"peerDependencies"`.
    fn save_peer(&self) -> bool {
        self.save_peer
    }

    pub(crate) fn cargo_dependency_kind(
        &self,
        has_node_packages: bool,
    ) -> miette::Result<CargoDependencyKind> {
        if self.save_optional || self.save_peer {
            return Err(miette::miette!(
                "crate: dependencies do not support --save-optional or --save-peer"
            ));
        }
        if self.save_build && has_node_packages {
            return Err(miette::miette!(
                "--save-build cannot be applied to Node.js packages in a mixed add"
            ));
        }
        let selected = [self.save_prod, self.save_dev, self.save_build]
            .into_iter()
            .filter(|selected| *selected)
            .count();
        if selected > 1 {
            return Err(miette::miette!(
                "crate: dependencies can be added to only one dependency table at a time"
            ));
        }
        Ok(if self.save_dev {
            CargoDependencyKind::Development
        } else if self.save_build {
            CargoDependencyKind::Build
        } else {
            CargoDependencyKind::Normal
        })
    }

    /// Convert the `--save-*` flags to an iterator of [`DependencyGroup`]
    /// which selects which target group to save to.
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        std::iter::empty()
            .chain(self.save_prod().then_some(DependencyGroup::Prod))
            .chain(self.save_dev().then_some(DependencyGroup::Dev))
            .chain(self.save_optional().then_some(DependencyGroup::Optional))
            .chain(self.save_peer().then_some(DependencyGroup::Peer))
    }

    /// The save target for the install layer: `Some` when a `--save-*`
    /// flag names it explicitly, `None` when pnpm infers it per package
    /// (an already-declared dependency is updated in the group it
    /// occupies; a new one lands in `dependencies`).
    fn save_target(&self) -> Option<Vec<DependencyGroup>> {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_build,
            save_peer,
            no_save_peer: _,
        } = self;
        (save_prod || save_dev || save_optional || save_build || save_peer)
            .then(|| self.dependency_groups().collect())
    }
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Names of the packages to add.
    #[clap(required = true)]
    pub package_names: Vec<String>,
    /// --save-prod, --save-dev, --save-optional, --save-peer
    #[clap(flatten)]
    pub dependency_options: AddDependencyOptions,
    /// `--cpu`, `--os`, and `--libc` filters for which optional dependencies are installed.
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,
    /// Saved dependencies will be configured with an exact version rather than using
    /// the default semver range operator.
    #[clap(short = 'E', long = "save-exact")]
    pub save_exact: bool,
    /// The prefix of the saved version range: `^` (default), `~`, `=` for an explicit exact pin, or empty for a bare exact version.
    #[clap(long = "save-prefix", value_name = "prefix")]
    pub save_prefix: Option<String>,
    /// Save the new dependency to the default catalog. Shorthand for `--save-catalog-name=default`.
    #[clap(long = "save-catalog")]
    pub save_catalog: bool,
    /// Save the new dependency to the named catalog `<name>`.
    #[clap(long = "save-catalog-name", value_name = "name")]
    pub save_catalog_name: Option<String>,
    /// Add the package as a configuration dependency.
    #[clap(long = "config")]
    pub config: bool,
    /// Only add the dependency if a workspace project provides it. The
    /// dependency is saved under the `workspace:` protocol and linked to
    /// that project.
    #[clap(long)]
    pub workspace: bool,
    /// Package names allowed to run lifecycle (build) scripts during this
    /// install, appended to `allowBuilds`. Prefix a name with `!` to deny
    /// its scripts instead. May be repeated.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,
    /// Dependencies are not downloaded. Only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
    /// Install the package globally, linking its bins into the global bin directory.
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Don't run lifecycle scripts of the added package or its dependencies.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,
    /// Force-enable lifecycle scripts for this invocation.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,
    /// Permit adding dependencies to a multi-package workspace root without `-w`.
    #[clap(
        long = "ignore-workspace-root-check",
        overrides_with = "no_ignore_workspace_root_check"
    )]
    pub ignore_workspace_root_check: bool,
    /// Keep the workspace-root safety check enabled.
    #[clap(
        long = "no-ignore-workspace-root-check",
        hide = true,
        overrides_with = "ignore_workspace_root_check"
    )]
    pub no_ignore_workspace_root_check: bool,
    /// Include optionalDependencies while materializing the updated project.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,
    /// Exclude optionalDependencies while materializing the updated project.
    #[clap(long = "no-optional", overrides_with = "optional")]
    pub no_optional: bool,
    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,
    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,
}

impl AddArgs {
    pub(crate) fn check_workspace_root(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        if config.recursive
            || config.workspace_root
            || resolve_bool_override(
                self.ignore_workspace_root_check,
                self.no_ignore_workspace_root_check,
                config.ignore_workspace_root_check,
            )
            || config.workspace_dir.as_deref() != Some(dir)
        {
            return Ok(());
        }
        let patterns = pnpm_workspace::read_workspace_manifest(dir)
            .into_diagnostic()?
            .map(|manifest| pnpm_workspace::workspace_package_patterns(&manifest));
        if patterns.as_ref().is_some_and(|patterns| patterns.len() > 1) {
            return Err(AddError::AddingToRoot.into());
        }
        Ok(())
    }

    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_scripts = resolve_bool_override(
            self.ignore_scripts,
            self.no_ignore_scripts,
            config.ignore_scripts,
        );
        config.ignore_workspace_root_check = resolve_bool_override(
            self.ignore_workspace_root_check,
            self.no_ignore_workspace_root_check,
            config.ignore_workspace_root_check,
        );
        config.optional = resolve_bool_override(self.optional, self.no_optional, config.optional);
        config.ignore_pnpmfile = self.ignore_pnpmfile || config.ignore_pnpmfile;
        config.force = self.force || config.force;
    }

    /// The `--config` selectors parsed into the `name → specifier` pairs to
    /// record, or `None` when `--config` was not passed.
    ///
    /// Callers must run this *before* [`State::init`]: that scaffolds a
    /// `package.json` on disk, so rejecting an invalid selector afterwards
    /// would leave a half-created project behind. A version-less selector
    /// resolves the `latest` tag, matching the default `add` behavior.
    pub(super) fn parse_config_dependencies(
        &self,
    ) -> miette::Result<Option<BTreeMap<String, String>>> {
        if !self.config {
            return Ok(None);
        }

        let mut added = BTreeMap::new();
        for package_name in &self.package_names {
            let parsed = parse_wanted_dependency(package_name);
            let Some(name) = parsed.alias else {
                return Err(miette::miette!(
                    "'{package_name}' is not a valid package name for a configuration dependency",
                ));
            };
            let specifier = parsed.bare_specifier.unwrap_or_else(|| "latest".to_string());
            added.insert(name, specifier);
        }
        Ok(Some(added))
    }

    /// The style that decides the saved range: `--save-exact` /
    /// `--save-prefix` layered over the `saveExact` and `savePrefix`
    /// settings, mirroring pnpm's `getRangeSpecStyle`.
    fn range_spec_style(&self, config: &Config) -> RangeSpecStyle {
        RangeSpecStyle::from_save_options(
            self.save_exact || config.save_exact,
            self.save_prefix.as_deref().or(config.save_prefix.as_deref()),
        )
    }

    /// The workspace packages `--workspace` links the added dependencies
    /// to, indexed by name and version. `Ok(None)` means the flag was not
    /// passed.
    pub(crate) fn workspace_link_targets(
        &self,
        config: &Config,
    ) -> miette::Result<Option<WorkspacePackages>> {
        workspace_link_root(self.workspace, config.workspace_dir.as_deref())?
            .map(|workspace_root| {
                recursive::discover_workspace_projects(workspace_root, config).map(
                    |(projects, _)| {
                        build_workspace_packages_map(Some(&projects)).unwrap_or_default()
                    },
                )
            })
            .transpose()
    }
}

/// The `workspace:` requests `--workspace` resolves in place of the
/// selectors the user typed: `foo` becomes `foo@workspace:*`, `foo@^1`
/// becomes `foo@workspace:^1`, and an explicit `workspace:` range is kept.
///
/// `--workspace` asks to link packages the workspace has, so a selector
/// naming one it does not have is an error rather than a registry
/// fallback.
fn workspace_selectors(
    selectors: &[String],
    workspace_packages: &WorkspacePackages,
) -> Result<Vec<String>, AddError> {
    selectors
        .iter()
        .map(|selector| {
            let parsed = parse_wanted_dependency(selector);
            let Some(name) = parsed.alias else {
                return Err(AddError::NoPkgNameInSpec { selector: selector.clone() });
            };
            if !workspace_packages.contains_key(&name) {
                return Err(AddError::WorkspacePackageNotFound { name });
            }
            Ok(match parsed.bare_specifier {
                None => format!("{name}@workspace:*"),
                Some(range) if range.starts_with("workspace:") => selector.clone(),
                Some(range) => format!("{name}@workspace:{range}"),
            })
        })
        .collect()
}

/// Honor `--allow-build`: reject any package the root project explicitly
/// disallows (`allowBuilds: false`), persist the allowed names to
/// `settings_dir`'s `pnpm-workspace.yaml`, and enable them for this
/// install. `settings_dir` is the workspace root, or the project
/// directory outside a workspace. Mirrors pnpm's `add` handler; shared by
/// the workspace and `--global` add paths.
pub(crate) fn apply_allow_build(
    config: &mut Config,
    allow_build: &[String],
    settings_dir: &Path,
) -> miette::Result<()> {
    if allow_build.is_empty() {
        return Ok(());
    }
    let mut allow_build_map: Vec<(&str, bool)> = Vec::with_capacity(allow_build.len());
    let mut allowed_only: Vec<&str> = Vec::new();
    for pkg in allow_build {
        let (name, allowed) = parse_allow_build_selector(pkg);
        if name.is_empty() {
            return Err(AllowBuildError::MissingPackage.into());
        }
        allow_build_map.push((name, allowed));
        if allowed {
            allowed_only.push(name);
        }
    }
    let overlap: Vec<&str> = allowed_only
        .into_iter()
        .filter(|pkg| config.allow_builds.get(*pkg) == Some(&false))
        .collect();
    if !overlap.is_empty() {
        return Err(AllowBuildError::OverridingIgnoredBuiltDependencies {
            dependencies: overlap.join(", "),
        }
        .into());
    }
    set_allow_builds(settings_dir, allow_build_map.iter().copied()).into_diagnostic()?;
    for (name, is_allow) in allow_build_map {
        config.allow_builds.insert(name.to_string(), is_allow);
    }
    Ok(())
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AddError {
    #[display(
        "Running this command will add the dependency to the workspace root, which might not be what you want - if you really meant it, make it explicit by running this command again with the -w flag (or --workspace-root). If you don't want to see this warning anymore, you may set the ignore-workspace-root-check setting to true."
    )]
    #[diagnostic(code(ERR_PNPM_ADDING_TO_ROOT))]
    AddingToRoot,

    #[display(
        "Cannot declare {request} as the package manager of a filtered selection of projects"
    )]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_MANAGER_IN_SELECTION),
        help(
            "Which package manager a project uses is declared in that project. Run the command in the project itself, without a filter."
        )
    )]
    PackageManagerInSelection {
        #[error(not(source))]
        request: String,
    },

    /// A `--workspace` selector named a package that no workspace project
    /// publishes.
    #[display(r#""{name}" not found in the workspace"#)]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND))]
    WorkspacePackageNotFound {
        #[error(not(source))]
        name: String,
    },

    /// A `--workspace` selector carried no package name to look up in the
    /// workspace, such as a bare path or URL.
    #[display(r#"Cannot update/install from workspace through "{selector}""#)]
    #[diagnostic(code(ERR_PNPM_NO_PKG_NAME_IN_SPEC))]
    NoPkgNameInSpec {
        #[error(not(source))]
        selector: String,
    },
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AllowBuildError {
    #[display(
        "The following dependencies are ignored by the root project, but are allowed to be built by the current command: {dependencies}"
    )]
    #[diagnostic(
        code(ERR_PNPM_OVERRIDING_IGNORED_BUILT_DEPENDENCIES),
        help(
            "If you are sure you want to allow those dependencies to run installation scripts, remove them from the allowBuilds list (or change their value to true)."
        )
    )]
    OverridingIgnoredBuiltDependencies { dependencies: String },

    #[display(
        "The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts."
    )]
    #[diagnostic(code(ERR_PNPM_ALLOW_BUILD_MISSING_PACKAGE))]
    MissingPackage,
}

#[cfg(test)]
mod tests;

mod execution;
