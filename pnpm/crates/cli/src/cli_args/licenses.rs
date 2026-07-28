use crate::cli_args::{
    deps_tree::{
        dep_types::{DepType, detect_dep_types},
        pkg_info::is_unsafe_path_component,
    },
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    sanitize::{sanitize, sanitize_inline},
};
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::Config;
use pacquet_lockfile::{Lockfile, PackageKey, PkgName, ResolvedDependencyMap};
use pacquet_package_is_installable::{
    SupportedArchitectures, WantedPlatformRef, inferred_platform, platform_is_supported,
};
use pacquet_package_manager::{
    AllowBuildPolicy, validate_virtual_store_slot_containment, virtual_store_layout_for_lockfile,
};
use pacquet_package_manifest::{
    extract_license, node_version_from_engines_runtime, safe_read_package_json_from_dir,
};
use pacquet_resolving_git_resolver::HostedGit;
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
};
use tabled::{builder::Builder, settings::Style};

mod license_resolver;

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
    /// Only dependencies in "dependencies"
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

#[derive(Debug, Clone, Copy)]
struct Include {
    dependencies: bool,
    dev_dependencies: bool,
    optional_dependencies: bool,
}

impl LicensesDependencyOptions {
    fn include(&self) -> Include {
        // Mirrored from pnpm `licenses` logic (and sbom.rs).
        let mut dependencies = !self.dev;
        let mut dev_dependencies = !self.prod;
        let mut optional_dependencies = !self.prod && !self.no_optional;

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

#[derive(Debug, Serialize)]
pub struct LicenseInfo {
    pub name: String,
    pub versions: Vec<String>,
    pub paths: Vec<String>,
    pub license: String,
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
        match self.params.first().map(String::as_str) {
            Some("list" | "ls") => {}
            Some(_) => return Err(LicensesError::UnknownSubcommand.into()),
            None => return Err(LicensesError::NoSubcommand.into()),
        }

        let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
        let lockfile = Lockfile::load_wanted_from_dir(lockfile_dir).into_diagnostic()?;
        let Some(lockfile) = lockfile else {
            if self.json {
                println!("{{}}");
            }
            return Ok(());
        };

        let importer_ids = if recursive {
            let mut importer_ids = Vec::new();
            let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
            let (projects, _) = discover_workspace_projects(workspace_root)?;
            let selection =
                select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
            for project_dir in selection.selected.keys() {
                let id = if project_dir == lockfile_dir {
                    ".".to_string()
                } else {
                    project_dir
                        .strip_prefix(lockfile_dir)
                        .ok()
                        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        .filter(|id| !id.is_empty())
                        .unwrap_or_else(|| ".".to_string())
                };
                importer_ids.push(id);
            }
            importer_ids
        } else {
            lockfile.importers.keys().cloned().collect()
        };

        let include = self.dependency_options.include();
        let belongs_to = collect_dependencies(
            &lockfile,
            importer_ids,
            include,
            config.supported_architectures.as_ref(),
            pacquet_detect_libc::host_platform(),
            pacquet_detect_libc::host_arch(),
            pacquet_graph_hasher::host_libc(),
        );
        let allow_build_policy = AllowBuildPolicy::from_config(config).into_diagnostic()?;
        let project_manifest = safe_read_package_json_from_dir(dir).into_diagnostic()?;
        let manifest_node_version =
            project_manifest.as_ref().and_then(node_version_from_engines_runtime);
        let effective_node_version =
            config.node_version.as_deref().or(manifest_node_version.as_deref());
        let layout = virtual_store_layout_for_lockfile(
            config,
            effective_node_version,
            lockfile.snapshots.as_ref(),
            lockfile.packages.as_ref(),
            Some(&allow_build_policy),
            Some(lockfile_dir),
        );
        validate_virtual_store_slot_containment(lockfile.snapshots.as_ref(), &layout)
            .into_diagnostic()?;

        let pkgs = lockfile.packages.as_ref();
        let mut dependencies = belongs_to
            .into_iter()
            .map(|(key, kind)| {
                let name = key.name.to_string();
                let version = pkgs
                    .and_then(|packages| packages.get(&key.without_peer()))
                    .and_then(|meta| meta.version.clone())
                    .unwrap_or_else(|| key.suffix.version().to_string());
                (key, kind, name, version)
            })
            .collect::<Vec<_>>();
        dependencies.sort_by(|left, right| {
            compare_package_names(&left.2, &right.2)
                .then_with(|| compare_versions(&left.3, &right.3))
                .then_with(|| left.0.to_string().cmp(&right.0.to_string()))
                .then_with(|| left.1.cmp(&right.1))
        });

        let mut results_by_license: IndexMap<String, BTreeMap<String, LicenseInfo>> =
            IndexMap::new();

        for (key, kind, name, version) in dependencies {
            let pkg_dir = layout.slot_dir(&key).join("node_modules").join(&name);
            let manifest = if is_unsafe_path_component(&name) {
                None
            } else {
                safe_read_package_json_from_dir(&pkg_dir).unwrap_or(None)
            };

            let license = match manifest.as_ref() {
                Some(manifest) => match extract_license(manifest) {
                    Some(license) if !license.to_ascii_lowercase().contains("see license") => {
                        license
                    }
                    manifest_license => {
                        license_resolver::resolve_license_from_dir(manifest_license, &pkg_dir)
                            .await
                            .unwrap_or_else(|| "Unknown".to_string())
                    }
                },
                None => "Unknown".to_string(),
            };
            let author = manifest.as_ref().and_then(extract_license_author);
            let homepage = manifest.as_ref().and_then(extract_license_homepage);
            let description = manifest
                .as_ref()
                .and_then(|m| m.get("description"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let path_str = pkg_dir.to_string_lossy().to_string();

            let license_group = results_by_license.entry(license.clone()).or_default();
            let info = license_group.entry(name.clone()).or_insert_with(|| LicenseInfo {
                name: name.clone(),
                versions: Vec::new(),
                paths: Vec::new(),
                license,
                belongs_to: kind,
                selected_version: version.clone(),
                author: author.clone(),
                homepage: homepage.clone(),
                description: description.clone(),
            });

            if select_newer_version(info, &version, kind) {
                info.author = author;
                info.homepage = homepage;
                info.description = description;
            }
            if !info.versions.contains(&version) {
                info.versions.push(version);
                info.paths.push(path_str);
            }
        }

        if self.json {
            let mut json_output: IndexMap<String, Vec<&LicenseInfo>> = IndexMap::new();
            for (lic, group) in &results_by_license {
                let mut infos: Vec<&LicenseInfo> = group.values().collect();
                infos.sort_by(|a, b| compare_package_names(&a.name, &b.name));
                json_output.insert(lic.clone(), infos);
            }

            let json = serde_json::to_string_pretty(&json_output)
                .map_err(|e| miette::miette!("Failed to serialize json: {}", e))?;
            println!("{json}");
            return Ok(());
        }

        if results_by_license.is_empty() {
            return Ok(());
        }

        let mut header: Vec<String> = vec!["Package".to_string(), "License".to_string()];
        if self.long {
            header.push("Details".to_string());
        }

        let mut builder = Builder::default();
        builder.push_record(header);

        let mut all_packages: Vec<&LicenseInfo> =
            results_by_license.values().flat_map(|g| g.values()).collect();
        all_packages.sort_by(|a, b| compare_package_names(&a.name, &b.name));

        for info in all_packages {
            let mut row =
                vec![render_package_name(info), sanitize_inline(&info.license).into_owned()];
            if self.long {
                let mut details = Vec::new();
                if let Some(author) = &info.author {
                    details.push(author.clone());
                }
                if let Some(desc) = &info.description {
                    details.push(desc.clone());
                }
                if let Some(home) = &info.homepage {
                    details.push(home.clone());
                }
                row.push(sanitize(&details.join("\n")).into_owned());
            }
            builder.push_record(row);
        }

        let mut table = builder.build();
        table.with(Style::modern());
        println!("{table}");

        Ok(())
    }
}

fn collect_dependencies(
    lockfile: &Lockfile,
    importer_ids: impl IntoIterator<Item = impl AsRef<str>>,
    include: Include,
    supported_architectures: Option<&SupportedArchitectures>,
    current_os: &str,
    current_cpu: &str,
    current_libc: &str,
) -> HashMap<PackageKey, BelongsTo> {
    let mut belongs_to: HashMap<PackageKey, BelongsTo> = HashMap::new();
    let mut stack: Vec<(PackageKey, BelongsTo)> = Vec::new();

    for id in importer_ids {
        let Some(importer) =
            lockfile.importers.get(id.as_ref()).or_else(|| lockfile.root_project())
        else {
            continue;
        };
        let mut queue_deps = |deps: Option<&ResolvedDependencyMap>, kind: BelongsTo| {
            if let Some(deps) = deps {
                for (alias, spec) in deps {
                    if let Some(key) = spec.version.resolved_key(alias) {
                        stack.push((key, kind));
                    }
                }
            }
        };

        if include.dependencies {
            queue_deps(importer.dependencies.as_ref(), BelongsTo::Prod);
        }
        if include.dev_dependencies {
            queue_deps(importer.dev_dependencies.as_ref(), BelongsTo::Dev);
        }
        if include.optional_dependencies {
            queue_deps(importer.optional_dependencies.as_ref(), BelongsTo::Optional);
        }
    }

    let empty_snapshots = HashMap::new();
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty_snapshots);

    while let Some((key, kind)) = stack.pop() {
        if let Some(existing) = belongs_to.get(&key)
            && *existing <= kind
        {
            continue;
        }

        let snapshot = snapshots.get(&key);
        let package =
            lockfile.packages.as_ref().and_then(|packages| packages.get(&key.without_peer()));
        if snapshot.is_some_and(|snapshot| snapshot.optional)
            && package.is_some_and(|package| {
                let declared = WantedPlatformRef {
                    os: package.os.as_deref(),
                    cpu: package.cpu.as_deref(),
                    libc: package.libc.as_deref(),
                };
                let inferred =
                    (declared.os.is_none() || declared.cpu.is_none() || declared.libc.is_none())
                        .then(|| key.name.to_string())
                        .and_then(|name| inferred_platform(&name, declared));
                let wanted = inferred.as_ref().map_or(declared, |platform| WantedPlatformRef {
                    os: platform.os.as_deref(),
                    cpu: platform.cpu.as_deref(),
                    libc: platform.libc.as_deref(),
                });
                !platform_is_supported(
                    wanted,
                    supported_architectures,
                    current_os,
                    current_cpu,
                    current_libc,
                )
            })
        {
            continue;
        }

        belongs_to.insert(key.clone(), kind);

        if let Some(snapshot) = snapshot {
            let mut queue_children =
                |deps: Option<&HashMap<PkgName, pacquet_lockfile::SnapshotDepRef>>| {
                    if let Some(deps) = deps {
                        for (name, dep_ref) in deps {
                            if let Some(child_key) = dep_ref.resolve(name) {
                                stack.push((child_key, kind));
                            }
                        }
                    }
                };

            queue_children(snapshot.dependencies.as_ref());
            if include.optional_dependencies {
                queue_children(snapshot.optional_dependencies.as_ref());
            }
        }
    }

    let dep_types = detect_dep_types(lockfile);
    for (key, belongs_to) in &mut belongs_to {
        *belongs_to = if dep_types.get(key) == Some(&DepType::DevOnly) {
            BelongsTo::Dev
        } else {
            BelongsTo::Prod
        };
    }

    belongs_to
}

fn version_is_newer(candidate: &str, selected: &str) -> bool {
    compare_versions(candidate, selected).is_gt()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn compare_package_names(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(package_name_collation_weight)
        .cmp(right.bytes().map(package_name_collation_weight))
        .then_with(|| {
            left.bytes()
                .zip(right.bytes())
                .find_map(|(left, right)| {
                    if left == right || !left.eq_ignore_ascii_case(&right) {
                        None
                    } else if left.is_ascii_lowercase() {
                        Some(Ordering::Less)
                    } else {
                        Some(Ordering::Greater)
                    }
                })
                .unwrap_or_else(|| left.cmp(right))
        })
}

fn package_name_collation_weight(byte: u8) -> u8 {
    match byte {
        b'_' => 0,
        b'-' => 1,
        b'.' => 2,
        b'@' => 3,
        b'/' => 4,
        b'~' => 5,
        byte => byte.to_ascii_lowercase().saturating_add(6),
    }
}

fn select_newer_version(
    info: &mut LicenseInfo,
    candidate_version: &str,
    candidate_belongs_to: BelongsTo,
) -> bool {
    if !version_is_newer(candidate_version, &info.selected_version) {
        return false;
    }
    info.belongs_to = candidate_belongs_to;
    info.selected_version = candidate_version.to_string();
    true
}

fn render_package_name(info: &LicenseInfo) -> String {
    let name = sanitize_inline(&info.name);
    let suffix = match info.belongs_to {
        BelongsTo::Prod | BelongsTo::Optional => return name.into_owned(),
        BelongsTo::Dev => "(dev)",
    };
    format!("{} {}", name, suffix.if_supports_color(Stream::Stdout, |text| text.dimmed()))
}

fn extract_license_author(manifest: &serde_json::Value) -> Option<String> {
    match manifest.get("author")? {
        serde_json::Value::String(author) => {
            if author.is_empty() {
                return Some(String::new());
            }
            let name_end = author.find(['(', '<']).unwrap_or(author.len());
            let name = author[..name_end].trim();
            (!name.is_empty()).then(|| name.to_string())
        }
        serde_json::Value::Object(author) => {
            author.get("name").and_then(serde_json::Value::as_str).map(ToString::to_string)
        }
        _ => None,
    }
}

fn extract_license_homepage(manifest: &serde_json::Value) -> Option<String> {
    if let Some(homepage) =
        manifest.get("homepage").and_then(serde_json::Value::as_str).filter(|url| !url.is_empty())
    {
        return Some(if url::Url::parse(homepage).is_ok() {
            homepage.to_string()
        } else {
            format!("http://{homepage}")
        });
    }

    let repository = match manifest.get("repository")? {
        serde_json::Value::String(repository) => repository,
        serde_json::Value::Object(repository) => {
            repository.get("url").and_then(serde_json::Value::as_str)?
        }
        _ => return None,
    };
    HostedGit::package_docs_url(repository)
}

#[cfg(test)]
mod tests;
