//! `pnpm view node`: Node.js as `pnpm add node` installs it, from the
//! release index of the configured Node.js mirror. The npm package named
//! `node` stays reachable as `node@npm:node`.

use super::ViewError;
use crate::{
    cli_args::registry_client::build_registry_client,
    engine_pm::selector::tool_install_selector,
};
use pnpm_config::{
    Config,
    Tool,
};
use pnpm_engine_runtime_node_resolver::{
    NodeRelease,
    fetch_node_releases,
    get_node_mirror,
    parse_node_specifier,
    pick_node_version,
};
use serde_json::{
    Map,
    Value,
    json,
};

/// The `runtime:` specifier `package_spec` asks for when it names Node.js
/// rather than an npm package, such as `latest` for `node` or `rc/24` for
/// `node@runtime:rc/24`.
pub(super) fn node_runtime_specifier(package_spec: &str) -> Option<String> {
    let selector = tool_install_selector(package_spec);
    selector
        .as_deref()
        .unwrap_or(package_spec)
        .strip_prefix("node@runtime:")
        .map(ToString::to_string)
}

/// Node.js release info in the shape [`super::render`] reads, and the
/// mirror it came from.
pub(super) struct NodeView {
    pub info: Value,
    pub mirror: String,
}

/// Fetch the mirror's release index and assemble the info of the release
/// `specifier` picks.
pub(super) async fn fetch_node_view(config: &Config, specifier: &str) -> miette::Result<NodeView> {
    let parsed = parse_node_specifier(specifier).map_err(miette::Report::new)?;
    let mirror = node_mirror(config, &parsed.release_channel);
    let http_client = build_registry_client(config)?;
    let releases = fetch_node_releases(&http_client, &config.auth_headers, Some(&mirror))
        .await
        .map_err(miette::Report::new)?;
    let picked = pick_node_version(&releases, &parsed.version_specifier)
        .and_then(|version| {
            releases
                .iter()
                .find(|release| release.version == version)
        })
        .ok_or_else(|| ViewError::PackageNotFound {
            name: "node".to_string(),
            spec: specifier.to_string(),
        })?;
    Ok(NodeView { info: assemble_node_info(&releases, picked), mirror })
}

/// The mirror of `release_channel`, as installing Node.js reads it from
/// the config.
pub(crate) fn node_mirror(config: &Config, release_channel: &str) -> String {
    let channels = config.tool_channel_mirrors(Tool::Node);
    get_node_mirror(
        config.tool_mirror(Tool::Node),
        channels.get(release_channel).map(String::as_str),
        Some(&config.node_download_mirrors),
        release_channel,
    )
}

/// The picked release's index row under `name` and `version`, with the
/// index-wide `versions`, `dist-tags`, and `time` a packument carries.
/// Versions are listed oldest first, as a packument lists them.
fn assemble_node_info(releases: &[NodeRelease], picked: &NodeRelease) -> Value {
    let mut info = Map::new();
    info.insert("name".to_string(), json!("node"));
    info.insert("version".to_string(), json!(picked.version));
    for (key, value) in &picked.index_entry {
        info.entry(key.as_str()).or_insert_with(|| value.clone());
    }

    let versions: Vec<&str> = releases
        .iter()
        .rev()
        .map(|release| release.version.as_str())
        .collect();
    info.insert("versionsCount".to_string(), json!(versions.len()));
    info.insert("versions".to_string(), json!(versions));

    let dist_tags: Map<String, Value> = [("latest", "latest"), ("lts", "lts")]
        .into_iter()
        .filter_map(|(tag, selector)| {
            pick_node_version(releases, selector).map(|version| (tag.to_string(), json!(version)))
        })
        .collect();
    info.insert("distTags".to_string(), Value::Object(dist_tags.clone()));
    info.insert("dist-tags".to_string(), Value::Object(dist_tags));

    let time: Map<String, Value> = releases
        .iter()
        .rev()
        .filter_map(|release| {
            let date = release.index_entry.get("date")?.as_str()?;
            Some((release.version.clone(), json!(format!("{date}T00:00:00.000Z"))))
        })
        .collect();
    info.insert("time".to_string(), Value::Object(time));

    Value::Object(info)
}

#[cfg(test)]
mod tests;
