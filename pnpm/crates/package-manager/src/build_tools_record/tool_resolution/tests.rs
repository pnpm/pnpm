use super::{WantedTool, locked_tool};
use crate::build_tools_record::ToolCatalog;
use pnpm_lockfile::PackageKey;

fn catalog(keys: &[&str]) -> ToolCatalog {
    keys.iter()
        .map(|key| {
            let text = format!(
                "{{integrity: sha512-mQ==, tarball: https://registry.npmjs.org/x/-/{key}.tgz}}",
            );
            let resolution = serde_saphyr::from_str(&text).expect("parse resolution");
            let metadata = super::tool_metadata_of(resolution);
            (key.parse::<PackageKey>().expect("parse key"), metadata)
        })
        .collect()
}

fn locked(catalog: &ToolCatalog, name: &'static str, version_spec: &str) -> Option<String> {
    locked_tool(catalog, &WantedTool { name, version_spec }).map(|(key, _)| key.to_string())
}

#[test]
fn the_highest_satisfying_recorded_version_is_reused() {
    let catalog =
        catalog(&["yarn@tool:1.22.19", "yarn@tool:1.22.22", "yarn@tool:4.9.2", "npm@tool:10.9.2"]);
    assert_eq!(locked(&catalog, "yarn", "1"), Some("yarn@tool:1.22.22".to_string()));
    assert_eq!(locked(&catalog, "yarn", ">=2"), Some("yarn@tool:4.9.2".to_string()));
    assert_eq!(locked(&catalog, "npm", "^10"), Some("npm@tool:10.9.2".to_string()));
    assert_eq!(locked(&catalog, "npm", "^11"), None);
    // A dist-tag commits to no version, so it resolves.
    assert_eq!(locked(&catalog, "npm", "latest"), None);
}
