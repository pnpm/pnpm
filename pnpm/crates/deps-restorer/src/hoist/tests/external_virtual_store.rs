use super::{
    HoistInputs, build_hoist_graph, create_matcher, get_hoisted_dependencies, kinds_for,
    make_lockfile_data, pats, root_direct_deps,
};
use pnpm_modules_yaml::HoistKind;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

#[test]
fn external_store_preserves_root_versions_and_private_exclusions() {
    let (snapshots, packages) =
        make_lockfile_data(&[("types", "1.0.0", &[], false), ("types", "2.0.0", &[], false)]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let mut direct = root_direct_deps(&[("types", "types", "2.0.0")]);
    let workspace_deps = direct.shift_remove(".").unwrap();
    direct.insert("workspace".to_string(), workspace_deps);
    direct.extend(root_direct_deps(&[("types", "types", "1.0.0")]));
    let skipped = HashSet::new();
    for (private_patterns, expected) in [
        (pats(["*"]), vec![("types".to_string(), HoistKind::Private)]),
        (pats(["*", "!types"]), vec![]),
        (vec![], vec![]),
    ] {
        let result = get_hoisted_dependencies(&HoistInputs {
            graph: &graph,
            direct_deps_by_importer: &direct,
            skipped: &skipped,
            private_pattern: create_matcher(&private_patterns),
            public_pattern: create_matcher(&pats(["*"])),
            hoisted_workspace_packages: None,
            hoist_root_dependencies: true,
        })
        .expect("non-empty graph");
        dbg!(&result.hoisted_dependencies);
        assert_eq!(kinds_for(&result.hoisted_dependencies, "types@1.0.0"), expected);
        assert_eq!(kinds_for(&result.hoisted_dependencies, "types@2.0.0"), vec![]);
    }
}
