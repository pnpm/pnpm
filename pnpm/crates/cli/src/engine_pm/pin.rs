//! Record which package manager a project uses.
//!
//! A package manager is not a dependency of the project it installs, so
//! naming one in `pnpm add` writes the field that declares it instead of a
//! dependency entry. Which field that is depends on the package manager:
//! Yarn is started from a project pin by corepack, which reads only the
//! original `packageManager` field, so a Yarn pin goes there, written the
//! way `corepack use` writes it. Every other package manager is recorded
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
/// version — optionally carrying corepack's integrity — for
/// `packageManager`; [`resolve_project_pin`] produces the right one. A
/// bare `pnpm add npm` asks for npm without asking for a version, so none
/// is invented for it.
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
        Channel::Registry { package } => {
            let resolved = resolve_release(config, pm, package, spec).await?;
            let integrity = corepack_integrity(package, resolved.manifest.as_deref());
            match integrity {
                Some(integrity) => format!("{}+{integrity}", resolved.version),
                None => resolved.version,
            }
        }
        Channel::Binary(BinaryChannel::Bun | BinaryChannel::Yarn) => {
            resolve_yarn_binary_version(config, spec).await?
        }
    };
    Ok(Some(reference))
}

/// Corepack's `+<algorithm>.<hash>` build for a resolved release, when it
/// can be known without fetching anything.
///
/// Corepack pins the artifact it downloads. For Yarn Classic that artifact
/// is the npm tarball, so the hash is the integrity the registry already
/// published and pnpm already has. Yarn Berry is downloaded from
/// `repo.yarnpkg.com` as a single file instead — not the npm package pnpm
/// installs — so its hash is not derivable from registry metadata, and a
/// pin without one is what corepack itself accepts (it then verifies the
/// release the same way pnpm does, through npm's signature).
fn corepack_integrity(package: &str, manifest: Option<&Value>) -> Option<String> {
    if package != YARN_CLASSIC_PACKAGE {
        return None;
    }
    let integrity = manifest?.get("dist")?.get("integrity")?.as_str()?;
    let integrity: ssri::Integrity = integrity.parse().ok()?;
    let (algorithm, hex) = integrity.to_hex();
    (algorithm == ssri::Algorithm::Sha512).then(|| format!("sha512.{hex}"))
}

/// The npm package Yarn Classic is published as, whose tarball corepack
/// downloads for the `<2` line.
const YARN_CLASSIC_PACKAGE: &str = "yarn";

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
        &pnpm_network::NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
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
