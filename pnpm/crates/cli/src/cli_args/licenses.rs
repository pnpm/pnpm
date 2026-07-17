use crate::cli_args::recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects};
use clap::Args;
use miette::IntoDiagnostic;
use pacquet_config::Config;
use pacquet_lockfile::{Lockfile, PackageKey, PkgName, ResolvedDependencyMap};
use pacquet_package_manifest::{extract_author, extract_homepage, safe_read_package_json_from_dir};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use tabled::{builder::Builder, settings::Style};

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
        let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
        let lockfile = Lockfile::load_wanted_from_dir(lockfile_dir).into_diagnostic()?;
        let Some(lockfile) = lockfile else {
            if self.json {
                println!("{{}}");
            }
            return Ok(());
        };

        let mut importer_ids = Vec::new();

        if recursive {
            let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
            let (projects, _) = discover_workspace_projects(workspace_root)?;
            let selection =
                select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
            for project_dir in selection.selected.keys() {
                let id = if project_dir == lockfile_dir {
                    ".".to_string()
                } else {
                    project_dir.strip_prefix(lockfile_dir)
                        .ok()
                        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        .filter(|id| !id.is_empty())
                        .unwrap_or_else(|| ".".to_string())
                };
                importer_ids.push(id);
            }
        } else {
            let importer_id = if dir == lockfile_dir {
                ".".to_string()
            } else {
                dir.strip_prefix(lockfile_dir)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| ".".to_string())
            };
            importer_ids.push(importer_id);
        }

        let include = self.dependency_options.include();
        let mut belongs_to: HashMap<PackageKey, BelongsTo> = HashMap::new();
        let mut stack: Vec<(PackageKey, BelongsTo)> = Vec::new();

        for id in importer_ids {
            let Some(importer) = lockfile.importers.get(&id).or_else(|| lockfile.root_project()) else {
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
        let mut seen = HashSet::new();

        while let Some((key, kind)) = stack.pop() {
            if let Some(existing) = belongs_to.get(&key)
                && *existing <= kind
            {
                continue;
            }

            belongs_to.insert(key.clone(), kind);

            if !seen.insert((key.clone(), kind)) {
                continue;
            }

            if let Some(snapshot) = snapshots.get(&key) {
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

        let project_root_dir = dir.to_path_buf();
        let effective_vsd = config.effective_virtual_store_dir();
        let virtual_store_dir = if effective_vsd.is_absolute() {
            effective_vsd.to_path_buf()
        } else {
            project_root_dir.join(effective_vsd)
        };
        let virtual_store_dir_max_length = config.virtual_store_dir_max_length as usize;

        let mut results_by_license: BTreeMap<String, BTreeMap<String, LicenseInfo>> =
            BTreeMap::new();

        let pkgs = lockfile.packages.as_ref();

        for (key, _kind) in belongs_to {
            let name = key.name.to_string();
            let mut version = key.suffix.version().to_string();
            if let Some(pkgs) = pkgs
                && let Some(meta) = pkgs.get(&key.without_peer())
                && let Some(v) = &meta.version
            {
                version.clone_from(v);
            }

            let store_name = key.to_virtual_store_name(virtual_store_dir_max_length);
            
            let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(&name);
            let is_unsafe = store_name.contains("..") || name.contains("..") || std::path::Path::new(&store_name).is_absolute() || std::path::Path::new(&name).is_absolute();
            let manifest = if is_unsafe {
                None
            } else {
                safe_read_package_json_from_dir(&pkg_dir).unwrap_or(None)
            };


            let license = manifest
                .as_ref()
                .and_then(|m| m.get("license"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let author = manifest.as_ref().and_then(extract_author);
            let homepage = manifest.as_ref().and_then(extract_homepage);
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
                author,
                homepage,
                description,
            });

            if !info.versions.contains(&version) {
                info.versions.push(version);
            }
            if !info.paths.contains(&path_str) {
                info.paths.push(path_str);
            }
        }

        for group in results_by_license.values_mut() {
            for info in group.values_mut() {
                info.versions.sort();
                info.paths.sort();
            }
        }

        if self.json {
            let mut json_output: BTreeMap<String, Vec<&LicenseInfo>> = BTreeMap::new();
            for (lic, group) in &results_by_license {
                let mut infos: Vec<&LicenseInfo> = group.values().collect();
                infos.sort_by(|a, b| a.name.cmp(&b.name));
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
        all_packages.sort_by(|a, b| a.name.cmp(&b.name));

        for info in all_packages {
            let mut row = vec![info.name.clone(), info.license.clone()];
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
                row.push(details.join("\n"));
            }
            builder.push_record(row);
        }

        let mut table = builder.build();
        table.with(Style::modern());
        println!("{table}");

        Ok(())
    }
}

#[cfg(test)]
mod tests;
