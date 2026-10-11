//! Resolve the tools a build environment asks for into lockfile entries.

use super::{BuildToolRecord, BuildToolRecorder, RecordBuildToolsError};
use pnpm_git_fetcher::{BuildEnvironment, PreferredPm};
use pnpm_lockfile::{Lockfile, PackageKey, PackageMetadata, Prefix};
use pnpm_resolving_resolver_base::{ResolveResult, WantedDependency};
use pnpm_tool_protocol::{PackageManagerSource, TOOL_PROTOCOL, package_manager_source};

/// A tool a build needs: its name in the lockfile and the range it must
/// satisfy.
struct WantedTool<'a> {
    name: &'static str,
    version_spec: &'a str,
}

impl WantedTool<'_> {
    /// The npm package the tool is published as, or `None` for one that
    /// ships as platform archives (Node.js, Bun, Yarn 6 and later).
    fn registry_package(&self) -> Option<&'static str> {
        match package_manager_source(self.name, self.version_spec)? {
            PackageManagerSource::Registry(package) => Some(package),
            PackageManagerSource::Archive => None,
        }
    }

    fn describe(&self) -> String {
        format!("{}@{}", self.name, self.version_spec)
    }
}

impl BuildToolRecorder<'_> {
    /// The record of `environment`'s tools. A tool `lockfile` already
    /// records at a satisfying version is reused, the way a regular
    /// dependency's locked version is.
    pub(super) async fn resolve_tools(
        &self,
        lockfile: &Lockfile,
        environment: &BuildEnvironment,
    ) -> Result<BuildToolRecord, RecordBuildToolsError> {
        let mut record = BuildToolRecord::new();
        for tool in wanted_tools(environment) {
            let entry = match locked_tool(lockfile, &tool) {
                Some(entry) => entry,
                None => self.resolve_tool(&tool).await?,
            };
            let alias = tool.name.parse().expect("a tool name is a valid package name");
            record.insert(alias, entry);
        }
        Ok(record)
    }

    async fn resolve_tool(
        &self,
        tool: &WantedTool<'_>,
    ) -> Result<(PackageKey, PackageMetadata), RecordBuildToolsError> {
        let registry_package = tool.registry_package();
        let wanted = match registry_package {
            Some(package) => WantedDependency {
                alias: Some(package.to_string()),
                bare_specifier: Some(tool.version_spec.to_string()),
                ..WantedDependency::default()
            },
            None => WantedDependency {
                alias: Some(tool.name.to_string()),
                bare_specifier: Some(format!("{TOOL_PROTOCOL}{}", tool.version_spec)),
                ..WantedDependency::default()
            },
        };
        let resolve_error =
            |reason: String| RecordBuildToolsError::Resolve { tool: tool.describe(), reason };
        let result = self.resolver
            .resolve(&wanted, self.resolve_options)
            .await
            .map_err(|error| resolve_error(error.to_string()))?
            .ok_or_else(|| RecordBuildToolsError::NoMatch { tool: tool.describe() })?;
        if let Some(violation) = &result.policy_violation {
            return Err(resolve_error(violation.reason.clone()));
        }
        let version = resolved_version(&result, registry_package)
            .ok_or_else(|| RecordBuildToolsError::NoMatch { tool: tool.describe() })?;
        let key = format!("{}@{TOOL_PROTOCOL}{version}", tool.name)
            .parse()
            .map_err(|_| RecordBuildToolsError::NoMatch { tool: tool.describe() })?;
        Ok((key, tool_metadata(result)))
    }
}

/// The tools `environment` asks for. pnpm is not one of them: the running
/// pnpm, or the version the package pins, prepares the package as it
/// does without a record.
fn wanted_tools(environment: &BuildEnvironment) -> Vec<WantedTool<'_>> {
    let package_manager = &environment.package_manager;
    let package_manager = (package_manager.pm != PreferredPm::Pnpm).then(|| WantedTool {
        name: package_manager.pm.name(),
        version_spec: package_manager.version_spec.as_deref().unwrap_or("latest"),
    });
    let node = environment.node
        .as_deref()
        .map(|version_spec| WantedTool { name: "node", version_spec });
    package_manager
        .into_iter()
        .chain(node)
        .collect()
}

/// The highest version of `tool` that `lockfile` records and the tool's
/// range admits, from the same source the range selects.
fn locked_tool(
    lockfile: &Lockfile,
    tool: &WantedTool<'_>,
) -> Option<(PackageKey, PackageMetadata)> {
    let range = node_semver::Range::parse(tool.version_spec).ok()?;
    let source = package_manager_source(tool.name, tool.version_spec);
    lockfile.packages
        .as_ref()?
        .iter()
        .filter(|(key, _)| key.suffix.prefix() == Prefix::Tool && key.name.to_string() == tool.name)
        .filter_map(|(key, metadata)| Some((key.suffix.version_semver()?, key, metadata)))
        .filter(|(version, _, _)| {
            version.satisfies(&range)
                && package_manager_source(tool.name, &version.to_string()) == source
        })
        .max_by(|(left, _, _), (right, _, _)| left.cmp(right))
        .map(|(_, key, metadata)| (key.clone(), metadata.clone()))
}

/// The exact version `result` resolved to. A registry package must be the
/// one asked for: an alias resolving to another package is no answer.
fn resolved_version(result: &ResolveResult, registry_package: Option<&str>) -> Option<String> {
    if let Some(package) = registry_package {
        let name_ver = result.package.name_ver.as_ref()?;
        return (name_ver.name.to_string() == package).then(|| name_ver.suffix.to_string());
    }
    let (_, version) = result.id.as_str().rsplit_once('@')?;
    Some(pnpm_tool_protocol::strip_tool_protocol(version).unwrap_or(version).to_string())
}

fn tool_metadata(result: ResolveResult) -> PackageMetadata {
    PackageMetadata {
        resolution: result.resolution,
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}
