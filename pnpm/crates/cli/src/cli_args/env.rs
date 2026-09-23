//! `pacquet env` — the deprecated Node.js-only front end to
//! [`super::runtime`], kept because pnpm still ships it.

use super::{
    add::AddRequest,
    global::{global_dirs, handle_global_add, handle_global_remove},
    registry_client::build_registry_client,
};
use crate::shim_dispatch::{ShimTarget, native_shim_target, remove_native_shim};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_cmd_shim::remove_bin as remove_cmd_shim;
use pnpm_config::{Config, Tool};
use pnpm_engine_runtime_node_resolver::{
    get_node_mirror, parse_node_specifier, resolve_node_versions_with_auth,
};
use pnpm_global::find_global_package;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{Reporter, emit_global_warning};
use std::{fs, path::Path};

/// Manage Node.js versions.
#[derive(Debug, Args)]
pub struct EnvArgs {
    /// Manage Node.js versions globally.
    #[clap(short = 'g', long)]
    pub global: bool,

    /// Accepted for surface parity with pnpm, which declares the option
    /// but never reads it.
    #[clap(long, hide = true)]
    pub remote: bool,

    /// Subcommand (`use`, `list`, `remove`) and its arguments.
    pub params: Vec<String>,
}

/// Emitted before `env use` does anything else, matching where pnpm warns.
const DEPRECATION_WARNING: &str =
    r#""pnpm env use" is deprecated. Use "pnpm runtime set node <version> -g" instead."#;

/// Emitted before `env remove` does anything else, matching where pnpm warns.
const REMOVE_DEPRECATION_WARNING: &str =
    r#""pnpm env remove" is deprecated. Use "pnpm remove -g node" instead."#;

/// Errors raised by `pacquet env`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EnvError {
    #[display("Please specify the subcommand")]
    #[diagnostic(
        code(ERR_PNPM_ENV_NO_SUBCOMMAND),
        help("Supported subcommands are: use, list, remove")
    )]
    NoSubcommand,

    #[display("This subcommand is not known")]
    #[diagnostic(
        code(ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND),
        help("Supported subcommands are: use, list, remove")
    )]
    UnknownSubcommand,

    #[display(
        "Unable to manage Node.js because pnpm was not installed using the standalone installation script"
    )]
    #[diagnostic(
        code(ERR_PNPM_CANNOT_MANAGE_NODE),
        help(
            "If you want to manage Node.js with pnpm, you need to remove any Node.js that was installed by other tools, then install pnpm using one of the standalone scripts that are provided on the installation page: https://pnpm.io/installation"
        )
    )]
    CannotManageNode,

    #[display(r#""pnpm env use <version>" can only be used with the "--global" option currently"#)]
    #[diagnostic(code(ERR_PNPM_NOT_IMPLEMENTED_YET))]
    LocalUseUnsupported,

    #[display(
        r#""pnpm env remove <version>" can only be used with the "--global" option currently"#
    )]
    #[diagnostic(code(ERR_PNPM_NOT_IMPLEMENTED_YET))]
    LocalRemoveUnsupported,

    #[display(
        r#""pnpm env {subcommand} --global <version>" requires a Node.js version to be specified"#
    )]
    #[diagnostic(code(ERR_PNPM_MISSING_NODE_VERSION))]
    MissingNodeVersion { subcommand: &'static str },
}

/// What [`EnvArgs`] resolved its parameters to.
///
/// The subcommands need different resources — the global config and
/// the install pipeline versus a registry client — so parsing is split
/// from running and the dispatcher picks the path.
#[derive(Debug)]
pub enum EnvSubcommand {
    Use { package_name: String },
    List { version_spec: Option<String> },
    Remove { versions: Vec<String> },
}

impl EnvArgs {
    fn parse_use<Reporter: self::Reporter>(&self) -> Result<EnvSubcommand, EnvError> {
        emit_global_warning::<Reporter>(DEPRECATION_WARNING);
        if !self.global {
            return Err(EnvError::LocalUseUnsupported);
        }
        let version = self.params
            .get(1)
            .map(|version| version.trim())
            .filter(|version| !version.is_empty())
            .ok_or(EnvError::MissingNodeVersion { subcommand: "use" })?;
        Ok(EnvSubcommand::Use { package_name: format!("node@runtime:{version}") })
    }

    fn parse_remove<Reporter: self::Reporter>(&self) -> Result<EnvSubcommand, EnvError> {
        emit_global_warning::<Reporter>(REMOVE_DEPRECATION_WARNING);
        if !self.global {
            return Err(EnvError::LocalRemoveUnsupported);
        }
        let versions: Vec<String> = self.params
            .iter()
            .skip(1)
            .map(|version| version.trim().to_string())
            .filter(|version| !version.is_empty())
            .collect();
        if versions.is_empty() {
            return Err(EnvError::MissingNodeVersion { subcommand: "remove" });
        }
        Ok(EnvSubcommand::Remove { versions })
    }

    pub fn subcommand<Reporter: self::Reporter>(
        self,
        config: &Config,
    ) -> Result<EnvSubcommand, EnvError> {
        let Some(subcommand) = self.params.first() else {
            return Err(EnvError::NoSubcommand);
        };
        if self.global && config.global_bin.is_none() {
            return Err(EnvError::CannotManageNode);
        }
        match subcommand.as_str() {
            "use" => self.parse_use::<Reporter>(),
            "remove" | "rm" | "uninstall" | "un" => self.parse_remove::<Reporter>(),
            "list" | "ls" => Ok(EnvSubcommand::List {
                version_spec: self.params
                    .get(1)
                    .map(|spec| spec.trim())
                    .filter(|spec| !spec.is_empty())
                    .map(ToOwned::to_owned),
            }),
            _ => Err(EnvError::UnknownSubcommand),
        }
    }

    /// Installs the runtime the same way
    /// [`super::runtime::RuntimeArgs::run_global`] does, so the deprecated
    /// spelling and its replacement cannot diverge.
    pub async fn run_use<Reporter: self::Reporter + 'static>(
        package_name: String,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        let request = AddRequest::from(package_name.as_str());
        Box::pin(handle_global_add::<Reporter>(
            config,
            std::slice::from_ref(&request),
            RangeSpecStyle::Major,
            config.supported_architectures.clone(),
            // A runtime install has no user packages, so no `--allow-build`.
            &[],
            dir,
        ))
        .await
    }

    /// Oldest first, so the newest version ends up next to the prompt.
    ///
    /// An absent selector reads as the empty one, which the mirror
    /// resolver treats as `latest` — a bare `pnpm env list` prints the
    /// newest version alone, not the whole index. Verified against the
    /// TypeScript CLI, which passes `''` rather than `undefined`.
    pub async fn run_list(version_spec: Option<String>, config: &Config) -> miette::Result<String> {
        let specifier = parse_node_specifier(version_spec.as_deref().unwrap_or_default())
            .map_err(miette::Report::new)?;
        let channels = config.tool_channel_mirrors(Tool::Node);
        let mirror = get_node_mirror(
            config.tool_mirror(Tool::Node),
            channels.get(&specifier.release_channel).map(String::as_str),
            Some(&config.node_download_mirrors),
            &specifier.release_channel,
        );
        let http_client = build_registry_client(config)?;
        let mut versions = resolve_node_versions_with_auth(
            &http_client,
            &config.auth_headers,
            Some(specifier.version_specifier.as_str()),
            Some(&mirror),
        )
        .await
        .map_err(miette::Report::new)?;
        versions.reverse();
        Ok(versions.join("\n"))
    }

    pub async fn run_remove<Reporter: self::Reporter + 'static>(
        versions: Vec<String>,
        config: &'static Config,
        _dir: &Path,
    ) -> miette::Result<()> {
        let (global_pkg_dir, global_bin_dir) = global_dirs(config)?;
        let mut removed_something = false;

        if let Some(pkg) = find_global_package(&global_pkg_dir, "node").into_diagnostic()? {
            let installed = pnpm_global::installed_versions(&pkg.install_dir);
            if let Some(installed_ver) = installed.get("node")
                && versions
                    .iter()
                    .any(|target_version| matches_node_version(installed_ver, target_version))
            {
                handle_global_remove::<Reporter>(config, &["node".to_string()])?;
                removed_something = true;
            }
        }

        let mut removed_names = std::collections::HashSet::new();
        let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();

        if cleanup_pnpm_home(pnpm_home.as_deref(), &versions, &mut removed_names).into_diagnostic()?
        {
            removed_something = true;
        }

        if cleanup_bin_links(&global_bin_dir, &removed_names).into_diagnostic()? {
            removed_something = true;
        }

        if !removed_something {
            return Err(super::global::GlobalError::PkgNotFound {
                param: format!("node (version {})", versions.join(", ")),
            }
            .into());
        }

        Ok(())
    }
}

fn matches_node_version(actual: &str, requested: &str) -> bool {
    actual == requested || actual.starts_with(&format!("{requested}."))
}

fn cleanup_legacy_nodejs_dir(
    nodejs_dir: &Path,
    versions: &[String],
    removed_names: &mut std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    let entries = match fs::read_dir(nodejs_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let mut removed = false;
    for entry in entries.flatten() {
        let name = entry
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !versions.iter().any(|target_version| matches_node_version(&name, target_version)) {
            continue;
        }
        fs::remove_dir_all(entry.path())?;
        removed_names.insert(name);
        removed = true;
    }
    Ok(removed)
}

fn is_path_in_removed_names(
    target: &Path,
    removed_names: &std::collections::HashSet<String>,
) -> bool {
    let target_str = target.to_string_lossy();
    removed_names
        .iter()
        .any(|name| {
            target_str
                .split(['/', '\\'])
                .any(|part| part == name)
        })
}

fn cleanup_nodejs_current(
    pnpm_home: &Path,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    let nodejs_current = pnpm_home.join("nodejs_current");
    if !nodejs_current.is_symlink() {
        return Ok(false);
    }
    let is_dangling = !nodejs_current.exists();
    let points_to_removed = fs::read_link(&nodejs_current)
        .is_ok_and(|target| is_path_in_removed_names(&target, removed_names));
    if is_dangling || points_to_removed {
        fs::remove_file(&nodejs_current)?;
        return Ok(true);
    }
    Ok(false)
}

fn is_symlink_removed(
    bin_path: &Path,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    if !bin_path.is_symlink() {
        return Ok(false);
    }
    if !bin_path.exists() {
        return Ok(true);
    }
    let target = fs::read_link(bin_path)?;
    Ok(is_path_in_removed_names(&target, removed_names))
}

fn is_native_shim_removed(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    match native_shim_target(global_bin_dir, bin_name) {
        Ok(Some(ShimTarget::Installed(target))) => {
            Ok(!target.exists() || is_path_in_removed_names(&target, removed_names))
        }
        Ok(None | Some(ShimTarget::Virtual(_))) => Ok(false),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn is_cmd_shim_removed(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    for ext in [".cmd", ".ps1"] {
        let shim = global_bin_dir.join(format!("{bin_name}{ext}"));
        let content = match fs::read_to_string(&shim) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        let points_to_removed = content
            .split(['"', '\'', '\\', '/', ' ', '\t', '\r', '\n'])
            .any(|segment| removed_names.contains(segment));
        if points_to_removed {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cleanup_single_bin_link(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    let bin_path = global_bin_dir.join(bin_name);
    let should_remove = is_symlink_removed(&bin_path, removed_names)?
        || is_native_shim_removed(global_bin_dir, bin_name, removed_names)?
        || is_cmd_shim_removed(global_bin_dir, bin_name, removed_names)?;

    if should_remove {
        remove_cmd_shim(&bin_path)?;
        remove_native_shim(global_bin_dir, bin_name)?;
        for ext in [".cmd", ".ps1"] {
            let shim = global_bin_dir.join(format!("{bin_name}{ext}"));
            if shim.exists() {
                fs::remove_file(&shim)?;
            }
        }
        if bin_path.exists() || bin_path.is_symlink() {
            fs::remove_file(&bin_path)?;
        }
        return Ok(true);
    }
    Ok(false)
}

fn cleanup_bin_links(
    global_bin_dir: &Path,
    removed_names: &std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    let mut removed = false;
    for bin_name in ["node", "npm", "npx"] {
        if cleanup_single_bin_link(global_bin_dir, bin_name, removed_names)? {
            removed = true;
        }
    }
    Ok(removed)
}

fn cleanup_pnpm_home(
    pnpm_home: Option<&Path>,
    versions: &[String],
    removed_names: &mut std::collections::HashSet<String>,
) -> std::io::Result<bool> {
    let Some(pnpm_home) = pnpm_home else {
        return Ok(false);
    };
    let dir_removed =
        cleanup_legacy_nodejs_dir(&pnpm_home.join("nodejs"), versions, removed_names)?;
    let link_removed = cleanup_nodejs_current(pnpm_home, removed_names)?;
    Ok(dir_removed || link_removed)
}

#[cfg(test)]
mod tests;
