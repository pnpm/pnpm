//! Record which package manager a project uses.
//!
//! A package manager is not a dependency of the project it installs, so
//! naming one in `pnpm add` writes the field that declares it instead of a
//! dependency entry. Which field that is depends on the package manager:
//! Yarn is started from a project pin by corepack, which reads only the
//! original `packageManager` field, so a Yarn pin goes there, as the
//! exact version that field takes. Every other package manager is recorded
//! in `devEngines.packageManager`, the field that holds a range and that
//! pnpm's own package-manager check prefers.
//!
//! Only one of the two is ever left behind: they are two declarations of
//! the same thing, and a project whose fields disagree is one corepack
//! refuses to run.

use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_package_manifest::package_manager_spec::is_version_request;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::{Map, Value};

use crate::engine_pm::{
    channel::{BinaryChannel, Channel, PackageManager},
    resolve::resolve_release,
};

/// The package manager `request` declares, and the version it asks for.
///
/// `None` is an ordinary install request. pnpm itself is one: its pin
/// makes the next command switch the running CLI, which is
/// `pnpm self-update`'s job to do deliberately. So is a specifier that
/// locates a package to install under the package manager's name —
/// `yarn@npm:@yarnpkg/cli-dist`, `yarn@yarnpkg/berry#main` — rather than
/// asking for a released version of the package manager itself.
pub(crate) fn declared_package_manager(request: &str) -> Option<(PackageManager, Option<String>)> {
    let parsed = parse_wanted_dependency(request);
    if parsed.bare_specifier.as_deref().is_some_and(|spec| !is_version_request(spec)) {
        return None;
    }
    let pm =
        PackageManager::parse(parsed.alias.as_deref()?).filter(|pm| *pm != PackageManager::Pnpm)?;
    Some((pm, parsed.bare_specifier))
}

/// Declare in `manifest` that the project uses `pm` at `reference`,
/// replacing whatever package manager it declared before.
///
/// `reference` is a range for `devEngines.packageManager` and an exact
/// version for `packageManager`; [`resolve_project_pin`] produces the
/// right one. A bare `pnpm add npm` asks for npm without asking for a
/// version, so none is invented for it.
pub(crate) fn record_package_manager_pin(
    manifest: &mut Map<String, Value>,
    pm: PackageManager,
    reference: Option<&str>,
) {
    let reference = reference.map(str::trim).filter(|reference| !reference.is_empty());
    if pm == PackageManager::Yarn {
        clear_dev_engines_package_manager(manifest);
        let pin = match reference {
            Some(reference) => format!("{}@{reference}", pm.name()),
            None => pm.name().to_string(),
        };
        manifest.insert("packageManager".to_string(), Value::String(pin));
        return;
    }

    manifest.remove("packageManager");
    let mut entry = Map::new();
    entry.insert("name".to_string(), Value::String(pm.name().to_string()));
    if let Some(reference) = reference {
        entry.insert("version".to_string(), Value::String(reference.to_string()));
    }
    let dev_engines = manifest.entry("devEngines").or_insert_with(|| Value::Object(Map::new()));
    if !dev_engines.is_object() {
        *dev_engines = Value::Object(Map::new());
    }
    let dev_engines = dev_engines.as_object_mut().expect("just made it an object");
    dev_engines.insert("packageManager".to_string(), Value::Object(entry));
}

/// Drop the `devEngines.packageManager` declaration, and `devEngines`
/// itself once it declares nothing else.
fn clear_dev_engines_package_manager(manifest: &mut Map<String, Value>) {
    let Some(dev_engines) = manifest.get_mut("devEngines").and_then(Value::as_object_mut) else {
        return;
    };
    dev_engines.remove("packageManager");
    if dev_engines.is_empty() {
        manifest.remove("devEngines");
    }
}

/// The reference to record for `pm` at `version_spec`.
///
/// Yarn's field takes an exact version — corepack rejects a range there —
/// so the request is resolved against the same channel that would install
/// it. The other package managers keep the range the user asked for.
///
/// Only the version is recorded, never corepack's `+<algorithm>.<hash>`
/// build. Corepack is what installs a release from this pin, and for the
/// line it fetches from npm it derives that hash from the registry's
/// signed metadata anyway; recording one here would add a second copy of
/// it to a field pnpm itself never verifies, for the two to drift apart.
pub(crate) async fn resolve_project_pin(
    config: &'static Config,
    pm: PackageManager,
    version_spec: Option<&str>,
) -> miette::Result<Option<String>> {
    if pm != PackageManager::Yarn {
        return Ok(version_spec.map(ToString::to_string));
    }
    let version_spec = version_spec.map(str::trim).filter(|spec| !spec.is_empty());
    let spec = version_spec.unwrap_or("latest");
    let reference = match pm.channel(spec) {
        Channel::Registry { package } => resolve_release(config, pm, package, spec).await?.version,
        Channel::Binary(BinaryChannel::Bun | BinaryChannel::Yarn) => {
            resolve_yarn_binary_version(config, spec).await?
        }
    };
    Ok(Some(reference))
}

/// Resolve a Yarn line that ships as a platform archive rather than an npm
/// package. Its releases are listed by `yarnpkg/zpm`, not by a registry.
async fn resolve_yarn_binary_version(
    config: &Config,
    version_spec: &str,
) -> miette::Result<String> {
    let bootstrap = &config.package_manager_bootstrap;
    let client = pnpm_network::ThrottledClient::for_installs(
        &bootstrap.proxy,
        &bootstrap.tls,
        &bootstrap.tls_by_uri,
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err("build the network client to resolve the Yarn release")?;
    pnpm_engine_pm_yarn_resolver::resolve_yarn_version(&client, version_spec)
        .await
        .map_err(miette::Report::new)
}

/// How the recorded pin reads back, for the line `pnpm add` prints.
pub(crate) fn describe_pin(pm: PackageManager, reference: Option<&str>) -> String {
    match reference.map(str::trim).filter(|reference| !reference.is_empty()) {
        Some(reference) => format!("{}@{reference}", pm.name()),
        None => pm.name().to_string(),
    }
}

#[cfg(test)]
mod tests;
