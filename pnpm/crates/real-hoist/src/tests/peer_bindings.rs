use super::{
    BTreeSet, HashMap, HoistOpts, Lockfile, ProjectSnapshot, Rc, SnapshotDepRef, SnapshotEntry,
    assert_eq, child_of, dep_key, empty_lockfile, hoist, nm_hoist, pkg_metadata_with_peer,
    pkg_name, resolved_dep, tree_node, tree_root, ver_peer,
};
use std::collections::BTreeMap;

#[test]
fn identical_peer_bindings_dedupe_despite_a_conflicting_ancestor() {
    for client_name in ["a-client", "z-client"] {
        for root_schema in ["1.0.0", "2.0.0"] {
            let client_version = "1.0.0(schema@2.0.0)";
            let root_deps = HashMap::from([
                (pkg_name(client_name), resolved_dep(client_version)),
                (pkg_name("host"), resolved_dep("1.0.0")),
                (pkg_name("schema"), resolved_dep(root_schema)),
            ]);
            let host_deps = HashMap::from([
                (pkg_name(client_name), SnapshotDepRef::Plain(ver_peer(client_version))),
                (pkg_name("schema"), SnapshotDepRef::Plain(ver_peer("3.0.0"))),
            ]);
            let client_deps =
                HashMap::from([(pkg_name("schema"), SnapshotDepRef::Plain(ver_peer("2.0.0")))]);
            let lockfile = Lockfile {
                importers: HashMap::from([(
                    Lockfile::ROOT_IMPORTER_KEY.to_string(),
                    ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
                )]),
                packages: Some(HashMap::from([
                    (dep_key(client_name, "1.0.0"), pkg_metadata_with_peer("schema")),
                    (dep_key("host", "1.0.0"), pkg_metadata_with_peer(client_name)),
                ])),
                snapshots: Some(HashMap::from([
                    (
                        dep_key(client_name, client_version),
                        SnapshotEntry {
                            dependencies: Some(client_deps),
                            ..SnapshotEntry::default()
                        },
                    ),
                    (
                        dep_key("host", "1.0.0"),
                        SnapshotEntry { dependencies: Some(host_deps), ..SnapshotEntry::default() },
                    ),
                    (dep_key("schema", "1.0.0"), SnapshotEntry::default()),
                    (dep_key("schema", "2.0.0"), SnapshotEntry::default()),
                    (dep_key("schema", "3.0.0"), SnapshotEntry::default()),
                ])),
                ..empty_lockfile()
            };

            let result = hoist(&lockfile, &HoistOpts::default()).unwrap();
            dbg!(&result);
            let host = child_of(&result, "host");
            assert_eq!(
                host.dependencies.borrow().len(),
                1,
                "{client_name}, root schema {root_schema}: host keeps only its own schema",
            );
            assert!(
                child_of(&host, "schema").references.borrow().contains("schema@3.0.0"),
                "the host retains its conflicting regular dependency",
            );
            let client = child_of(&result, client_name);
            let schema = if root_schema == "2.0.0" {
                assert!(client.dependencies.borrow().is_empty());
                child_of(&result, "schema")
            } else {
                assert_eq!(client.dependencies.borrow().len(), 1);
                child_of(&client, "schema")
            };
            assert!(schema.references.borrow().contains("schema@2.0.0"));
        }
    }
}

#[test]
fn different_recorded_peer_bindings_stay_separate_when_ancestors_agree() {
    let schema1 = tree_node("schema", "schema@1.0.0", &[], vec![]);
    let schema2 = tree_node("schema", "schema@2.0.0", &[], vec![]);
    let root_client = tree_node("client", "client@1.0.0", &["schema"], vec![Rc::clone(&schema2)]);
    let nested_client = tree_node("client", "client@1.0.0", &["schema"], vec![schema1]);
    let host = tree_node("host", "host@1.0.0", &[], vec![Rc::clone(&schema2), nested_client]);
    let root = tree_root(vec![root_client, host, schema2]);

    let result = nm_hoist(&root, &HoistOpts::default());
    dbg!(&result);
    assert!(child_of(&result, "client").dependencies.borrow().is_empty());
    let nested_client = child_of(&child_of(&result, "host"), "client");
    assert!(
        child_of(&nested_client, "schema").references.borrow().contains("schema@1.0.0"),
        "the nested client's binding must not be replaced by its ancestor's schema@2.0.0",
    );
}

#[test]
fn a_nested_peer_provider_moves_with_its_package_into_a_free_root_slot() {
    let border_on_client = HoistOpts {
        hoisting_limits: BTreeMap::from([(
            ".@".to_string(),
            BTreeSet::from(["client".to_string()]),
        )]),
        ..HoistOpts::default()
    };
    for (root_schema, opts) in [(true, HoistOpts::default()), (false, border_on_client)] {
        let schema2 = tree_node("schema", "schema@2.0.0", &[], vec![]);
        let client = tree_node("client", "client@1.0.0(schema@2.0.0)", &["schema"], vec![schema2]);
        let host_a = tree_node("host-a", "host-a@1.0.0", &[], vec![Rc::clone(&client)]);
        let host_b = tree_node("host-b", "host-b@1.0.0", &[], vec![client]);
        let mut root_deps = vec![host_a, host_b];
        if root_schema {
            root_deps.push(tree_node("schema", "schema@1.0.0", &[], vec![]));
        }
        let root = tree_root(root_deps);

        let result = nm_hoist(&root, &opts);
        dbg!(&result);
        for host in ["host-a", "host-b"] {
            assert!(child_of(&result, host).dependencies.borrow().is_empty());
        }
        let client = child_of(&result, "client");
        assert!(child_of(&client, "schema").references.borrow().contains("schema@2.0.0"));
    }
}
