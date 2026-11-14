use crate::cli_args::{
    deps_tree::{
        dep_types::{DepType, detect_dep_types},
        pkg_info::is_unsafe_path_component,
    },
    install::resolve_bool_override,
    sanitize::{sanitize, sanitize_inline},
};
use clap::Args;
use dependencies::{
    collect_dependencies, compare_package_names, compare_versions, select_newer_version,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use owo_colors::{OwoColorize, Stream};
use pnpm_config::Config;
use pnpm_lockfile::{
    Lockfile, PackageKey, PeerEdgeOptions, PeerSatisfactionEdges, ResolvedDependencyMap,
};
use pnpm_package_is_installable::{
    InstallabilityOptions, WantedPlatformRef, platform_is_supported_with_inference,
};
use pnpm_package_manifest::{extract_license, safe_read_package_json_from_dir};
use pnpm_resolving_git_resolver::HostedGit;
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
};
use tabled::{builder::Builder, settings::Style};

mod license_resolver;
use license_resolver::{extract_license_author, extract_license_homepage};

#[derive(Debug, Args)]
pub struct LicensesArgs {
    /// Output the information in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Show more details (such as a link to the repo).
    #[clap(long)]
    pub long: bool,

    #[clap(flatten)]
    pub dependency_options: LicensesDependencyOptions,

    /// Subcommand and arguments.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
enum LicensesError {
    #[display("Please specify the subcommand")]
    #[diagnostic(
        code(ERR_PNPM_LICENCES_NO_SUBCOMMAND),
        help("Run `pnpm licenses --help` for available subcommands.")
    )]
    NoSubcommand,

    #[display("This subcommand is not known")]
    #[diagnostic(code(ERR_PNPM_LICENSES_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand,
}

#[derive(Debug, Args)]
pub struct LicensesDependencyOptions {
    /// Check only "dependencies" and "optionalDependencies"
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Only dependencies in "devDependencies"
    #[clap(short = 'D', long)]
    dev: bool,
    /// Don't check "optionalDependencies"
    #[clap(long = "no-optional")]
    no_optional: bool,
    /// Only dependencies in "optionalDependencies"
    #[clap(short = 'O', long)]
    optional: bool,
}

use pnpm_modules_yaml::IncludedDependencies as Include;

impl LicensesDependencyOptions {
    fn include(&self, include_optional: bool) -> Include {
        // Mirrored from pnpm `licenses` logic (and sbom.rs).
        let mut dependencies = !self.dev;
        let mut dev_dependencies = !self.prod;
        let mut optional_dependencies =
            resolve_bool_override(self.optional, self.no_optional, include_optional);

        if self.optional {
            dependencies = false;
            dev_dependencies = false;
            optional_dependencies = true;
        }

        Include { dependencies, dev_dependencies, optional_dependencies }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BelongsTo {
    Prod,
    Optional,
    Dev,
}

/// Package identity within a license bucket: `(name, registry_name)`.
type LicensePackageKey = (String, Option<String>);
type LicensesByGroup = IndexMap<String, BTreeMap<LicensePackageKey, LicenseInfo>>;

#[derive(Debug, Serialize)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the pnpm licenses JSON output format."
    )
)]
pub struct LicenseInfo {
    pub name: String,
    pub versions: Vec<String>,
    pub paths: Vec<String>,
    pub license: String,
    #[serde(rename = "registryName", skip_serializing_if = "Option::is_none")]
    pub registry_name: Option<String>,
    #[serde(skip)]
    belongs_to: BelongsTo,
    #[serde(skip)]
    selected_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl LicensesArgs {
    pub async fn run(
        self,
        config: &Config,
        dir: &std::path::Path,
        recursive: bool,
    ) -> miette::Result<()> {
        check_licenses_subcommand(self.params.first().map(String::as_str))?;

        let include = self.dependency_options.include(config.optional);
        let LicensedPackages { package_dirs, dependencies } =
            LicensedPackages::collect(config, dir, recursive, include)?;

        let results_by_license = group_by_license(&package_dirs, dependencies).await;

        if self.json {
            pnpm_diagnostics::println!("{}", render_licenses_json(&results_by_license)?);
            return Ok(());
        }

        if results_by_license.is_empty() {
            pnpm_diagnostics::println!("No licenses in packages found");
            return Ok(());
        }

        print_license_table(&results_by_license, self.long);
        Ok(())
    }
}

fn print_license_table(results_by_license: &LicensesByGroup, long: bool) {
    let mut header: Vec<String> = vec!["Package".to_string(), "License".to_string()];
    if long {
        header.push("Details".to_string());
    }

    let mut builder = Builder::default();
    builder.push_record(header);
    for info in sorted_license_infos(results_by_license) {
        let mut row = vec![render_package_name(info), sanitize_inline(&info.license).into_owned()];
        if long {
            row.push(render_license_details(info));
        }
        builder.push_record(row);
    }

    let mut table = builder.build();
    table.with(Style::modern());
    pnpm_diagnostics::println!("{table}");
}

/// `pnpm licenses` takes exactly one subcommand, `list` (or `ls`).
fn check_licenses_subcommand(subcommand: Option<&str>) -> Result<(), LicensesError> {
    match subcommand {
        Some("list" | "ls") => Ok(()),
        Some(_) => Err(LicensesError::UnknownSubcommand),
        None => Err(LicensesError::NoSubcommand),
    }
}

/// Collect each package's license, grouped by license and then by
/// `(name, registry_name)`.
async fn group_by_license(
    package_dirs: &[PackageDirs],
    dependencies: Vec<(usize, LicensedDependency)>,
) -> LicensesByGroup {
    let mut results_by_license = LicensesByGroup::new();
    for (lockfile_index, dependency) in dependencies {
        let dirs = package_dirs[lockfile_index].package_dirs(
            &dependency.key,
            &dependency.name,
            &dependency.version,
        );
        for pkg_dir in dirs.iter() {
            record_license(&mut results_by_license, &dependency, pkg_dir).await;
        }
    }
    results_by_license
}

async fn record_license(
    results_by_license: &mut LicensesByGroup,
    dependency: &LicensedDependency,
    pkg_dir: &std::path::Path,
) {
    let details = read_license_details(pkg_dir, &dependency.name).await;
    let path_str = pkg_dir.to_string_lossy().to_string();

    let license_group = results_by_license.entry(details.license.clone()).or_default();
    let group_key = (dependency.name.clone(), dependency.registry_name.clone());
    let info = license_group
        .entry(group_key)
        .or_insert_with(|| LicenseInfo {
            name: dependency.name.clone(),
            versions: Vec::new(),
            paths: Vec::new(),
            license: details.license,
            registry_name: dependency.registry_name.clone(),
            belongs_to: dependency.belongs_to,
            selected_version: dependency.version.clone(),
            author: details.author.clone(),
            homepage: details.homepage.clone(),
            description: details.description.clone(),
        });

    // The newest version of a package supplies the rendered details.
    if select_newer_version(info, &dependency.version, dependency.belongs_to) {
        info.author = details.author;
        info.homepage = details.homepage;
        info.description = details.description;
    }
    if !info.versions
        .iter()
        .any(|existing| existing == &dependency.version)
    {
        info.versions.push(dependency.version.clone());
    }
    if !info.paths.contains(&path_str) {
        info.paths.push(path_str);
    }
}

/// Every collected package, in name and registry order — the table lists
/// packages rather than grouping them by license.
fn sorted_license_infos(results_by_license: &LicensesByGroup) -> Vec<&LicenseInfo> {
    let mut all_packages: Vec<&LicenseInfo> = results_by_license
        .values()
        .flat_map(BTreeMap::values)
        .collect();
    all_packages.sort_by(|left, right| compare_license_infos(left, right));
    all_packages
}

fn compare_license_infos(left: &LicenseInfo, right: &LicenseInfo) -> Ordering {
    compare_package_names(&left.name, &right.name)
        .then_with(|| left.registry_name.cmp(&right.registry_name))
}

/// One package's license and the manifest fields `--long` renders.
struct LicenseDetails {
    license: String,
    author: Option<String>,
    homepage: Option<String>,
    description: Option<String>,
}

/// The package's declared license, falling back to a license file in its
/// directory when the manifest declares none or defers to one.
async fn read_license_details(pkg_dir: &std::path::Path, name: &str) -> LicenseDetails {
    let manifest = if is_unsafe_path_component(name) {
        None
    } else {
        safe_read_package_json_from_dir(pkg_dir).unwrap_or(None)
    };
    let Some(manifest) = manifest else {
        return LicenseDetails {
            license: "Unknown".to_string(),
            author: None,
            homepage: None,
            description: None,
        };
    };
    let license = match extract_license(&manifest) {
        Some(license) if !license.to_ascii_lowercase().contains("see license") => license,
        manifest_license => license_resolver::resolve_license_from_dir(manifest_license, pkg_dir)
            .await
            .unwrap_or_else(|| "Unknown".to_string()),
    };
    LicenseDetails {
        license,
        author: extract_license_author(&manifest),
        homepage: extract_license_homepage(&manifest),
        description: manifest
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string),
    }
}

fn render_licenses_json(results_by_license: &LicensesByGroup) -> miette::Result<String> {
    let mut json_output: IndexMap<String, Vec<&LicenseInfo>> = IndexMap::new();
    for (license, group) in results_by_license {
        let mut infos: Vec<&LicenseInfo> = group.values().collect();
        infos.sort_by(|left, right| compare_license_infos(left, right));
        json_output.insert(license.clone(), infos);
    }
    serde_json::to_string_pretty(&json_output)
        .map_err(|error| miette::miette!("Failed to serialize json: {}", error))
}

/// The `--long` details column: whichever of author, description and
/// homepage the package declares, one per line.
fn render_license_details(info: &LicenseInfo) -> String {
    let details = [info.author.as_ref(), info.description.as_ref(), info.homepage.as_ref()]
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    sanitize(&details.join("\n")).into_owned()
}

fn render_package_name(info: &LicenseInfo) -> String {
    let name = sanitize_inline(&info.name);
    let package_name = match info.registry_name.as_deref() {
        Some(registry_name) => {
            let alias = format!("({})", sanitize_inline(registry_name));
            format!("{name} {}", alias.if_supports_color(Stream::Stdout, |text| text.dimmed()))
        }
        None => name.into_owned(),
    };
    match info.belongs_to {
        BelongsTo::Prod | BelongsTo::Optional => package_name,
        BelongsTo::Dev => format!(
            "{package_name} {}",
            "(dev)".if_supports_color(Stream::Stdout, |text| text.dimmed()),
        ),
    }
}

#[cfg(test)]
mod tests;

mod dependencies;

mod package_dirs;
use package_dirs::PackageDirs;

mod lockfiles;
use lockfiles::{LicensedDependency, LicensedPackages};
