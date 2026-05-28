use pacquet_config::HoistingLimits;
use pacquet_lockfile::{Lockfile, ProjectSnapshot, ResolvedDependencyMap};
use pacquet_real_hoist::percent_encode_path;
use std::collections::{BTreeSet, HashMap};

/// Translate the user-facing [`HoistingLimits`] mode into the
/// `@yarnpkg/nm` hoister's per-locator border map (the shape
/// [`pacquet_real_hoist::HoistOpts::hoisting_limits`] consumes). A
/// name in a locator's set is a hoisting border: that node's
/// dependencies are not hoisted above it.
///
/// Ports pnpm's
/// [`getHoistingLimits`](https://github.com/pnpm/pnpm/blob/89812a9353/installing/linking/real-hoist/src/index.ts):
///
/// - [`HoistingLimits::None`] → empty map (hoist as far as possible).
/// - [`HoistingLimits::Workspaces`] → border every workspace package
///   (and the root's direct deps) at the root locator, so each
///   project's dependencies stay within that project.
/// - [`HoistingLimits::Dependencies`] → additionally border each
///   workspace package's own direct dependencies, so their
///   transitives stay nested beneath them.
///
/// Pacquet's hoister currently hoists into the single root importer
/// only, so it consults the `.@` entry; the per-importer entries the
/// `dependencies` mode emits are produced for parity and become
/// load-bearing once multi-level hoisting lands.
pub fn get_hoisting_limits(
    importers: &HashMap<String, ProjectSnapshot>,
    mode: HoistingLimits,
) -> pacquet_real_hoist::HoistingLimits {
    let mut limits = pacquet_real_hoist::HoistingLimits::new();
    if matches!(mode, HoistingLimits::None) {
        return limits;
    }

    // The root border accumulates the root's own direct deps plus
    // every (encoded) non-root importer id, regardless of iteration
    // order — `BTreeSet` makes the result deterministic even though
    // `importers` is a `HashMap`. Only stored under `.@` when a root
    // importer is present, matching upstream.
    let mut root_border: BTreeSet<String> = BTreeSet::new();
    let mut root_present = false;

    for (importer_id, importer) in importers {
        if importer_id == Lockfile::ROOT_IMPORTER_KEY {
            root_present = true;
            collect_direct_dep_names(importer, &mut root_border);
            continue;
        }

        root_border.insert(percent_encode_path(importer_id));
        if !matches!(mode, HoistingLimits::Dependencies) {
            // `workspaces` mode borders each package at the root only;
            // their own direct deps don't get a per-importer border.
            continue;
        }

        let mut importer_border: BTreeSet<String> = BTreeSet::new();
        collect_direct_dep_names(importer, &mut importer_border);
        limits.insert(
            format!("{}@workspace:{importer_id}", percent_encode_path(importer_id)),
            importer_border,
        );
    }

    if root_present {
        limits.insert(format!("{}@", Lockfile::ROOT_IMPORTER_KEY), root_border);
    }

    limits
}

/// Collect every direct-dependency alias of `importer` (across the
/// regular, dev, and optional groups) into `out`. Aliases are the
/// node names the hoister matches borders against.
fn collect_direct_dep_names(importer: &ProjectSnapshot, out: &mut BTreeSet<String>) {
    for group in [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ] {
        let Some(deps) = group else { continue };
        add_alias_names(deps, out);
    }
}

fn add_alias_names(deps: &ResolvedDependencyMap, out: &mut BTreeSet<String>) {
    for alias in deps.keys() {
        out.insert(alias.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::get_hoisting_limits;
    use pacquet_config::HoistingLimits;
    use pacquet_lockfile::{
        Lockfile, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
        ResolvedDependencySpec,
    };
    use std::collections::{BTreeSet, HashMap};

    fn project_with_deps(names: &[&str]) -> ProjectSnapshot {
        let mut deps = ResolvedDependencyMap::new();
        for name in names {
            // `get_hoisting_limits` reads only the alias keys; the spec
            // value is filled in just to satisfy the map's value type.
            deps.insert(
                name.parse::<PkgName>().expect("valid pkg name"),
                ResolvedDependencySpec {
                    specifier: "1.0.0".to_string(),
                    version: "1.0.0".parse::<PkgVerPeer>().expect("parse version").into(),
                },
            );
        }
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() }
    }

    fn root_only() -> HashMap<String, ProjectSnapshot> {
        let mut importers = HashMap::new();
        importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a", "b"]));
        importers
    }

    /// `none` (the default) produces no borders, so the hoister
    /// flattens as far as possible.
    #[test]
    fn none_mode_yields_no_borders() {
        assert!(get_hoisting_limits(&root_only(), HoistingLimits::None).is_empty());
    }

    /// For a single root project, every mode that limits hoisting
    /// borders the root's direct deps at the `.@` locator.
    #[test]
    fn root_direct_deps_are_bordered_under_dependencies_mode() {
        let limits = get_hoisting_limits(&root_only(), HoistingLimits::Dependencies);
        assert_eq!(limits.keys().cloned().collect::<Vec<_>>(), vec![".@".to_string()]);
        assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "b".to_string()]));
    }

    /// `workspaces` mode borders each workspace package (encoded id)
    /// and the root's direct deps at the root locator, with no
    /// per-importer entry.
    #[test]
    fn workspaces_mode_borders_packages_at_root() {
        let mut importers = HashMap::new();
        importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a"]));
        importers.insert("packages/foo".to_string(), project_with_deps(&["b"]));

        let limits = get_hoisting_limits(&importers, HoistingLimits::Workspaces);
        assert_eq!(limits.keys().cloned().collect::<Vec<_>>(), vec![".@".to_string()]);
        assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "packages%2Ffoo".to_string()]));
    }

    /// `dependencies` mode additionally borders each non-root
    /// importer's own direct deps under its workspace locator.
    #[test]
    fn dependencies_mode_borders_each_importer() {
        let mut importers = HashMap::new();
        importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a"]));
        importers.insert("packages/foo".to_string(), project_with_deps(&["b"]));

        let limits = get_hoisting_limits(&importers, HoistingLimits::Dependencies);
        let mut keys = limits.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            vec![".@".to_string(), "packages%2Ffoo@workspace:packages/foo".to_string()],
        );
        assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "packages%2Ffoo".to_string()]));
        assert_eq!(
            limits["packages%2Ffoo@workspace:packages/foo"],
            BTreeSet::from(["b".to_string()]),
        );
    }
}
