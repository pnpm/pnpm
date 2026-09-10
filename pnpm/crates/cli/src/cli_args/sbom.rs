//! `pacquet sbom` — generate a Software Bill of Materials.
//!
//! Ports pnpm's `sbom` command
//! (`pnpm11/deps/compliance/commands/src/sbom/sbom.ts`).

use crate::{
    State,
    cli_args::{
        install::resolve_bool_override,
        recursive::{
            AutoExcludeRoot, discover_workspace_projects, no_projects_matched_message,
            notice_workspace_dir, select_recursive_projects, selected_importer_ids,
        },
    },
};
use clap::Args;
use collection::collect_components;
use cyclonedx::{CycloneDxOpts, serialize_cyclonedx};
use indexmap::IndexMap;
use metadata::{
    base64_to_hex, build_purl, classify_license, extract_bugs_url, extract_repository,
    generate_uuid_v4, integrity_string, normalize_link_path, peer_names_from_manifest,
    platform_incompatible_optional, read_pkg_metadata_from_store, sanitize_package_name,
    sanitize_path_segment, tarball_url_for_component,
};
use pnpm_config::Config;
use pnpm_lockfile::{
    LazyLockfile, Lockfile, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    PkgNameVerPeer, SnapshotEntry,
};
use pnpm_package_is_installable::{
    InstallabilityOptions, WantedPlatformRef, platform_is_supported_with_inference,
};
use pnpm_package_manager::{importer_root_dir, validate_importer_id};
use pnpm_package_manifest::{extract_author, extract_homepage, safe_read_package_json_from_dir};
use spdx::serialize_spdx;
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Display,
    hash::Hash,
    io::Write,
    path::{Path, PathBuf},
};
use walk::{
    ImporterComponents, WalkContext, WalkStores, component_walk_context, walk_importer_components,
};
use workspace::{
    merged_dedicated_lockfile_state, required_sbom_lockfile, select_importer_ids,
    selectors_narrow_the_run, sorted_importer_ids,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SbomFormat {
    #[clap(name = "cyclonedx")]
    CycloneDx,
    #[clap(name = "spdx")]
    Spdx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SbomComponentType {
    Library,
    Application,
}

#[derive(Debug, Args)]
pub struct SbomArgs {
    /// The SBOM output format (required).
    #[clap(long = "sbom-format", value_enum)]
    pub format: SbomFormat,

    /// The component type for the root package (default: library).
    #[clap(long = "sbom-type", value_enum, default_value = "library")]
    pub sbom_type: SbomComponentType,

    /// The `CycloneDX` specification version (`1.5`, `1.6`, or `1.7`; default: `1.7`).
    /// Only valid with `--sbom-format cyclonedx`.
    #[clap(long = "sbom-spec-version")]
    pub spec_version: Option<String>,

    /// Only use lockfile data (skip reading from the store).
    #[clap(long)]
    pub lockfile_only: bool,

    /// Comma-separated list of SBOM authors (`CycloneDX` `metadata.authors`).
    #[clap(long = "sbom-authors")]
    pub authors: Option<String>,

    /// SBOM supplier name (`CycloneDX` `metadata.supplier`).
    #[clap(long = "sbom-supplier")]
    pub supplier: Option<String>,

    /// Only include production dependencies.
    #[clap(long, short = 'P', visible_alias = "production")]
    pub prod: bool,

    /// Only include dev dependencies.
    #[clap(long, short = 'D')]
    pub dev: bool,

    /// Exclude optional dependencies.
    #[clap(long = "no-optional", overrides_with = "optional")]
    pub no_optional: bool,

    /// Include optional dependencies.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,

    /// Exclude peer dependencies.
    #[clap(long = "exclude-peers")]
    pub exclude_peers: bool,

    /// Write SBOM to a file instead of stdout. Use `%s` for the
    /// package name and `%v` for the version.
    #[clap(long)]
    pub out: Option<String>,

    /// Generate a separate SBOM for each matched workspace package.
    #[clap(long)]
    pub split: bool,
}

struct IncludeFilter {
    dependencies: bool,
    dev_dependencies: bool,
    optional_dependencies: bool,
}

impl SbomArgs {
    fn include_filter(&self, include_optional: bool) -> IncludeFilter {
        IncludeFilter {
            dependencies: !self.dev,
            dev_dependencies: !self.prod,
            // pnpm's config reader clears `optional` for a dev-only run,
            // and leaves it alone for a production-only one.
            optional_dependencies: !self.dev
                && resolve_bool_override(self.optional, self.no_optional, include_optional),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepType {
    DevOnly,
    ProdOnly,
}

struct SbomComponent {
    name: String,
    version: String,
    purl: String,
    dep_type: DepType,
    integrity: Option<String>,
    tarball_url: Option<String>,
    license: Option<String>,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    bugs_url: Option<String>,
}

struct SbomRelationship {
    from: String,
    to: String,
}

struct SbomResult {
    root_name: String,
    root_version: String,
    root_type: SbomComponentType,
    root_license: Option<String>,
    root_description: Option<String>,
    root_author: Option<String>,
    root_repository: Option<String>,
    root_bugs_url: Option<String>,
    components: Vec<SbomComponent>,
    relationships: Vec<SbomRelationship>,
}

/// Resolve a lockfile importer key to the on-disk directory whose
/// `package.json` the SBOM reads, returning `None` when that directory does
/// not stay inside the lockfile dir. Mirrors pnpm's SBOM importer handling
/// (`sbom.ts`): `validate_importer_id` is the cheap lexical pre-filter, then
/// both the lockfile dir and the importer dir are canonicalized so a
/// *symlinked* importer directory can't resolve outside the workspace, and a
/// resolved path outside the root is skipped rather than read. `importer_id`
/// comes from an untrusted lockfile.
fn confined_importer_dir(lockfile_dir: &Path, importer_id: &str) -> Option<PathBuf> {
    if validate_importer_id(importer_id).is_err() {
        return None;
    }
    let lockfile_root = std::fs::canonicalize(lockfile_dir).ok()?;
    let importer_dir = std::fs::canonicalize(importer_root_dir(lockfile_dir, importer_id)).ok()?;
    importer_dir.starts_with(&lockfile_root).then_some(importer_dir)
}

impl SbomArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        let (state, virtual_store_dirs) = self.merged_state(state)?;
        self.check_spec_version()?;

        let include = self.include_filter(state.config.optional);
        let authors = self.author_list();

        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let all_importer_ids = sorted_importer_ids(lockfile);
        let all_count = all_importer_ids.len();
        let Some(importer_ids) = select_importer_ids(&state, all_importer_ids, lockfile.is_some())?
        else {
            return Ok(());
        };

        if self.splits_output(&importer_ids) {
            return self.write_split_sboms(
                &state,
                &include,
                &authors,
                &importer_ids,
                virtual_store_dirs.as_deref(),
            );
        }
        let filter_ids: Option<Vec<&str>> = (selectors_narrow_the_run(state.config)
            || importer_ids.len() < all_count)
            .then(|| importer_ids.iter().map(String::as_str).collect());
        let result = collect_components(
            &state,
            &include,
            self.sbom_type,
            self.exclude_peers,
            self.lockfile_only,
            filter_ids.as_deref(),
            virtual_store_dirs.as_deref(),
        )?;
        self.write_single_sbom(&result, &self.serialize(&result, &authors, false))
    }

    fn author_list(&self) -> Vec<String> {
        self.authors
            .as_deref()
            .map(|csv| {
                csv.split(',')
                    .map(|author| author.trim().to_string())
                    .filter(|author| !author.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One SBOM per importer: `--split`, or an `--out` template with a `%s`
    /// placeholder and several importers to fill it.
    fn splits_output(&self, importer_ids: &[String]) -> bool {
        self.split
            || (self.out.as_ref().is_some_and(|o| o.contains("%s")) && importer_ids.len() > 1)
    }

    fn write_single_sbom(&self, result: &SbomResult, output: &str) -> miette::Result<()> {
        let mut stdout = std::io::stdout();
        if let Some(out_template) = self.out.as_deref() {
            let file_path = sbom_output_path(out_template, result);
            write_sbom_file(&file_path, output)?;
            let _ = writeln!(stdout, "{file_path}");
        } else {
            let _ = write!(stdout, "{output}");
        }
        let _ = stdout.flush();
        Ok(())
    }

    /// A workspace whose projects keep their own lockfiles has no one
    /// lockfile to walk, so a run that spans several of them merges
    /// their lockfiles — and their virtual stores — first.
    fn merged_state(&self, state: State) -> miette::Result<(State, Option<Vec<PathBuf>>)> {
        let spans_several_projects =
            state.config.recursive || self.split || selectors_narrow_the_run(state.config);
        if state.config.shares_one_lockfile() || !spans_several_projects {
            return Ok((state, None));
        }
        let (state, virtual_store_dirs) = merged_dedicated_lockfile_state(state)?;
        Ok((state, Some(virtual_store_dirs)))
    }

    /// `--sbom-spec-version` names a `CycloneDX` version, so it applies to
    /// that format alone.
    fn check_spec_version(&self) -> miette::Result<()> {
        let Some(spec_ver) = self.spec_version.as_deref() else {
            return Ok(());
        };
        if self.format != SbomFormat::CycloneDx {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_SPEC_VERSION_UNSUPPORTED_FORMAT",
                "The --sbom-spec-version option is only supported with --sbom-format cyclonedx."
            ));
        }
        if !["1.5", "1.6", "1.7"].contains(&spec_ver) {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_INVALID_SPEC_VERSION",
                r#"Invalid CycloneDX spec version "{spec_ver}". Supported versions: 1.5, 1.6, 1.7."#
            ));
        }
        Ok(())
    }

    fn serialize(&self, result: &SbomResult, authors: &[String], compact: bool) -> String {
        match self.format {
            SbomFormat::CycloneDx => serialize_cyclonedx(&CycloneDxOpts {
                result,
                spec_version: self.spec_version.as_deref(),
                lockfile_only: self.lockfile_only,
                authors,
                supplier: self.supplier.as_deref(),
                compact,
            }),
            SbomFormat::Spdx => serialize_spdx(result, compact),
        }
    }

    /// One SBOM per selected project: written to the `%s`-templated
    /// paths, or streamed as NDJSON when there is no `--out`.
    fn write_split_sboms(
        &self,
        state: &State,
        include: &IncludeFilter,
        authors: &[String],
        importer_ids: &[String],
        virtual_store_dirs: Option<&[PathBuf]>,
    ) -> miette::Result<()> {
        if let Some(out) = self.out.as_deref()
            && !out.contains("%s")
        {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_OUT_MISSING_PLACEHOLDER",
                "When using --split with --out, the path must contain %s as a placeholder for the package name."
            ));
        }

        let compact = self.out.is_none();
        let mut ndjson_lines: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        let mut written_paths: HashSet<String> = HashSet::new();
        for importer_id in importer_ids {
            let filter = [importer_id.as_str()];
            let result = collect_components(
                state,
                include,
                self.sbom_type,
                self.exclude_peers,
                self.lockfile_only,
                Some(&filter),
                virtual_store_dirs,
            )?;
            // A project with no manifest of its own describes nothing.
            if result.root_name == "unknown" {
                continue;
            }
            let output = self.serialize(&result, authors, compact);
            let Some(out_template) = self.out.as_deref() else {
                ndjson_lines.push(output);
                continue;
            };
            let file_path = claim_sbom_output(out_template, &result, &mut written_paths)?;
            write_sbom_file(&file_path, &output)?;
            files.push(file_path);
        }

        self.print_split_outputs(&files, &ndjson_lines);
        Ok(())
    }

    fn print_split_outputs(&self, files: &[String], ndjson_lines: &[String]) {
        let mut stdout = std::io::stdout();
        if self.out.is_some() {
            let _ = writeln!(
                stdout,
                "Generated {} SBOMs:\n{}",
                files.len(),
                files.iter().map(|file| format!("  {file}")).collect::<Vec<_>>().join("\n"),
            );
        } else {
            let _ = write!(stdout, "{}", ndjson_lines.join("\n"));
        }
        let _ = stdout.flush();
    }
}

/// The path one package's SBOM renders to, filling the `%s` / `%v`
/// placeholders of the `--out` template.
fn sbom_output_path(out_template: &str, result: &SbomResult) -> String {
    let sanitized_name = sanitize_path_segment(&sanitize_package_name(&result.root_name));
    let sanitized_ver = sanitize_path_segment(&result.root_version);
    out_template.replace("%s", &sanitized_name).replace("%v", &sanitized_ver)
}

fn write_sbom_file(file_path: &str, output: &str) -> miette::Result<()> {
    let path = std::path::Path::new(file_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| miette::miette!("create directory for {file_path}: {err}"))?;
    }
    std::fs::write(path, output).map_err(|err| miette::miette!("write SBOM to {file_path}: {err}"))
}

#[cfg(test)]
mod tests;

/// Claim before writing so a colliding name cannot overwrite the first SBOM.
fn claim_sbom_output(
    out_template: &str,
    result: &SbomResult,
    written_paths: &mut HashSet<String>,
) -> miette::Result<String> {
    let file_path = sbom_output_path(out_template, result);
    if !written_paths.insert(file_path.clone()) {
        return Err(miette::miette!(
            code = "ERR_PNPM_SBOM_OUT_PATH_COLLISION",
            r#"Multiple workspace packages resolve to the same output path "{file_path}". Include %v in the --out pattern to disambiguate."#
        ));
    }
    Ok(file_path)
}

mod cyclonedx;

mod spdx;

mod workspace;

mod walk;

mod metadata;

mod collection;
