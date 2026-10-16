use super::{WantedTool, locked_tool, wanted_tools};
use crate::build_tools_record::ToolCatalog;
use pnpm_git_fetcher::{BuildEnvironment, PreferredPm, WantedPm};
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
    locked_tool(catalog, &WantedTool { name, version_spec, integrity: None })
        .map(|(key, _)| key.to_string())
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

#[test]
fn pnpm_builds_record_the_current_executable_package() {
    let environment = BuildEnvironment {
        package_manager: WantedPm { pm: PreferredPm::Pnpm, version_spec: None, pinned: false },
        package_manager_integrity: None,
        node: None,
    };

    let tools = wanted_tools(&environment);

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "pnpm");
    assert_eq!(tools[0].version_spec, pnpm_config::PNPM_VERSION);
    assert_eq!(tools[0].registry_package(), Some("@pnpm/exe"));
}

#[test]
fn package_manager_integrity_stays_with_its_build_tool() {
    let environment = BuildEnvironment {
        package_manager: WantedPm {
            pm: PreferredPm::Pnpm,
            version_spec: Some("12.8.2".to_string()),
            pinned: true,
        },
        package_manager_integrity: Some("sha512-dGVzdA==".to_string()),
        node: None,
    };

    let tools = wanted_tools(&environment);

    assert_eq!(tools[0].integrity, Some("sha512-dGVzdA=="));
}
