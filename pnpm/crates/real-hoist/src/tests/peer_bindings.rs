use super::{
    BTreeSet, HashMap, HoistOpts, HoisterTree, Lockfile, ProjectSnapshot, Rc, RcByPtr,
    SnapshotDepRef, SnapshotEntry, assert_eq, child_of, dep_key, empty_lockfile, hoist, nm_hoist,
    pkg_metadata_with_peer, pkg_name, resolved_dep, tree_node, tree_root, ver_peer,
};
use std::collections::BTreeMap;

#[test]
fn identical_transitive_peer_bindings_dedupe_despite_a_conflicting_ancestor() {
    for (client_name, root_version) in
        [("a-client", "1.0.0"), ("a-client", "2.0.0"), ("z-client", "1.0.0"), ("z-client", "2.0.0")]
    {
        for depth in [1, 2] {
            let schema =
                |version: &str| tree_node("schema", &format!("schema@{version}"), &[], vec![]);
            let client = make_transitive_peer_client(client_name, depth);
            let host = tree_node(
                "host",
                "host@1.0.0",
                &[client_name],
                vec![Rc::clone(&client), schema("3.0.0")],
            );
            let root = tree_root(vec![client, host, schema(root_version)]);

            let result = nm_hoist(&root, &HoistOpts::default());
            let host = child_of(&result, "host");
            dbg!(&host);
            assert_eq!(
                host.dependencies.borrow().len(),
                1,
                "{client_name}, root schema {root_version}, depth {depth}: host retains only its own schema",
            );
            assert!(child_of(&host, "schema").references.borrow().contains("schema@3.0.0"));
        }
    }
}

#[test]
fn deduplication_compares_every_transitive_peer_provider() {
    let bridge = |name: &str, version: &str| {
        let schema = tree_node("schema", &format!("schema@{version}"), &[], vec![]);
        tree_node(name, &format!("{name}@1.0.0(schema@{version})"), &["schema"], vec![schema])
    };
    let client = |second_version: &str| {
        tree_node(
            "client",
            "client@1.0.0",
            &["schema"],
            vec![bridge("bridge-a", "2.0.0"), bridge("bridge-b", second_version)],
        )
    };
    for root_version in ["2.0.0", "4.0.0"] {
        let host = tree_node("host", "host@1.0.0", &[], vec![client("4.0.0")]);
        let root = tree_root(vec![client(root_version), host]);
        let result = nm_hoist(&root, &HoistOpts::default());
        let host = child_of(&result, "host");
        dbg!(&host);
        assert_eq!(host.dependencies.borrow().is_empty(), root_version == "4.0.0");
    }
}

#[test]
fn an_inherited_transitive_peer_is_not_hidden_by_a_recorded_sibling_binding() {
    let schema = |version: &str| tree_node("schema", &format!("schema@{version}"), &[], vec![]);
    let client = |version: &str, cyclic: bool| {
        let recorded = tree_node(
            "recorded",
            &format!("recorded@1.0.0(schema@{version})"),
            &["schema"],
            vec![schema(version)],
        );
        let inherited = tree_node("inherited", "inherited@1.0.0", &["schema"], vec![]);
        if cyclic {
            let cycle = tree_node("cycle", "cycle@1.0.0", &["schema"], vec![Rc::clone(&inherited)]);
            inherited.dependencies
                .borrow_mut()
                .insert(RcByPtr(cycle));
        }
        tree_node("client", "client@1.0.0", &["schema"], vec![recorded, inherited])
    };
    for (cyclic, inherited_version) in
        [(false, "1.0.0"), (false, "3.0.0"), (true, "1.0.0"), (true, "3.0.0")]
    {
        for recorded_version in ["2.0.0", "4.0.0"] {
            let host = tree_node(
                "host",
                "host@1.0.0",
                &[],
                vec![client("2.0.0", cyclic), schema("3.0.0")],
            );
            let root =
                tree_root(vec![client(recorded_version, cyclic), host, schema(inherited_version)]);

            let result = nm_hoist(&root, &HoistOpts::default());
            let host = child_of(&result, "host");
            let keeps_client = host.dependencies
                .borrow()
                .iter()
                .any(|child| child.0.name == "client");
            assert_eq!(
                keeps_client,
                inherited_version != "3.0.0" || recorded_version != "2.0.0",
                "inherited schema {inherited_version}, recorded schema {recorded_version}, cyclic {cyclic}",
            );
        }
    }
}

#[test]
fn recorded_providers_do_not_mask_a_changed_or_missing_inherited_provider() {
    let schema = |version: &str| tree_node("schema", &format!("schema@{version}"), &[], vec![]);
    let client = || {
        let providers = ["1.0.0", "3.0.0"].map(|version| {
            let name = format!("provider-{version}");
            tree_node(&name, &format!("{name}@1.0.0"), &["schema"], vec![schema(version)])
        });
        tree_node(
            "client",
            "client@1.0.0",
            &["schema"],
            providers
                .into_iter()
                .chain([tree_node("inherited", "inherited@1.0.0", &["schema"], vec![])])
                .collect(),
        )
    };
    for root_version in [Some("1.0.0"), Some("3.0.0"), None] {
        for host_version in ["1.0.0", "3.0.0"] {
            let host = tree_node("host", "host@1.0.0", &[], vec![client(), schema(host_version)]);
            let mut roots = vec![client(), host];
            roots.extend(root_version.map(schema));
            let mut root = tree_root(roots);
            if root_version.is_none() {
                Rc::get_mut(&mut root).unwrap().peer_names.insert("schema".to_string());
            }
            let result = nm_hoist(&root, &HoistOpts::default());
            let host = child_of(&result, "host");
            let keeps_client = host.dependencies
                .borrow()
                .iter()
                .any(|child| child.0.name == "client");
            assert_eq!(
                keeps_client,
                root_version != Some(host_version),
                "root {root_version:?}, host {host_version}",
            );
        }
    }
}

#[test]
fn a_recorded_provider_binds_every_node_of_a_transitive_peer_cycle() {
    for provider_name in ["a-provider", "z-provider"] {
        let schema = tree_node("schema", "schema@2.0.0", &[], vec![]);
        let provider =
            tree_node(provider_name, "provider@1.0.0(schema@2.0.0)", &["schema"], vec![schema]);
        let bridge = tree_node("bridge", "bridge@1.0.0", &["schema"], vec![]);
        let client = tree_node(
            "client",
            "client@1.0.0(schema@2.0.0)",
            &["schema"],
            vec![Rc::clone(&bridge), provider],
        );
        bridge.dependencies
            .borrow_mut()
            .insert(RcByPtr(Rc::clone(&client)));
        let host = tree_node(
            "host",
            "host@1.0.0",
            &[],
            vec![Rc::clone(&client), tree_node("schema", "schema@3.0.0", &[], vec![])],
        );
        let root = tree_root(vec![client, host, tree_node("schema", "schema@1.0.0", &[], vec![])]);

        let result = nm_hoist(&root, &HoistOpts::default());
        assert_eq!(child_of(&result, "host").dependencies.borrow().len(), 1);
    }
}

#[test]
fn a_transitive_peer_cycle_without_a_provider_uses_the_ancestor() {
    let bridge = tree_node("bridge", "bridge@1.0.0", &["schema"], vec![]);
    let client = tree_node("client", "client@1.0.0", &["schema"], vec![Rc::clone(&bridge)]);
    bridge.dependencies
        .borrow_mut()
        .insert(RcByPtr(Rc::clone(&client)));
    let schema = tree_node("schema", "schema@2.0.0", &[], vec![]);
    let host = tree_node("host", "host@1.0.0", &[], vec![Rc::clone(&client)]);
    let root = tree_root(vec![client, host, schema]);

    let result = nm_hoist(&root, &HoistOpts::default());
    let host = child_of(&result, "host");
    assert_eq!(host.dependencies.borrow().len(), 0);
}

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

#[test]
fn a_transitive_peer_provider_moves_with_its_package_into_a_free_root_slot() {
    for client_name in ["a-client", "z-client"] {
        for depth in [1, 2] {
            let client = make_transitive_peer_client(client_name, depth);
            let host_a = tree_node("host-a", "host-a@1.0.0", &[], vec![Rc::clone(&client)]);
            let host_b = tree_node("host-b", "host-b@1.0.0", &[], vec![client]);
            let root =
                tree_root(vec![host_a, host_b, tree_node("schema", "schema@1.0.0", &[], vec![])]);

            let result = nm_hoist(&root, &HoistOpts::default());
            for host in ["host-a", "host-b"] {
                assert!(
                    child_of(&result, host).dependencies.borrow().is_empty(),
                    "{client_name}, depth {depth}: {host} shares the root client",
                );
            }
            assert_eq!(child_of(&result, client_name).name, client_name);
        }
    }
}

#[test]
fn a_peer_provider_hoisted_above_a_nested_root_lets_copies_share_its_slot() {
    let schema2 = tree_node("schema", "schema@2.0.0", &[], vec![]);
    let client2 =
        tree_node("client", "client@2.0.0(schema@2.0.0)", &["schema"], vec![Rc::clone(&schema2)]);
    let wrapper_a2 = tree_node("wrapper-a", "wrapper-a@2.0.0", &[], vec![Rc::clone(&client2)]);
    let wrapper_b2 = tree_node("wrapper-b", "wrapper-b@2.0.0", &[], vec![client2]);
    let host = tree_node("host", "host@1.0.0", &[], vec![wrapper_a2, wrapper_b2]);
    let root = tree_root(vec![
        tree_node("client", "client@1.0.0", &[], vec![]),
        schema2,
        tree_node("wrapper-a", "wrapper-a@1.0.0", &[], vec![]),
        tree_node("wrapper-b", "wrapper-b@1.0.0", &[], vec![]),
        host,
    ]);

    let result = nm_hoist(&root, &HoistOpts::default());
    dbg!(&result);
    let host = child_of(&result, "host");
    for wrapper in ["wrapper-a", "wrapper-b"] {
        assert!(child_of(&host, wrapper).dependencies.borrow().is_empty());
    }
    assert!(child_of(&host, "client").references.borrow().contains("client@2.0.0(schema@2.0.0)"));
}

#[test]
fn a_nearer_peer_variant_keeps_a_matching_copy_from_deduping_against_the_root() {
    let schema = |version: &str| tree_node("schema", &format!("schema@{version}"), &[], vec![]);
    let client = |peer: &str, deps| {
        tree_node("client", &format!("client@1.0.0(schema@{peer})"), &["schema"], deps)
    };
    let wrapper2 =
        tree_node("wrapper", "wrapper@2.0.0", &[], vec![client("2.0.0", vec![schema("2.0.0")])]);
    let host = tree_node(
        "host",
        "host@1.0.0",
        &[],
        vec![client("1.0.0", vec![schema("1.0.0"), wrapper2])],
    );
    let root = tree_root(vec![
        client("2.0.0", vec![schema("2.0.0")]),
        tree_node("wrapper", "wrapper@1.0.0", &[], vec![]),
        host,
    ]);

    let result = nm_hoist(&root, &HoistOpts::default());
    dbg!(&result);
    let host = child_of(&result, "host");
    assert!(child_of(&host, "client").references.borrow().contains("client@1.0.0(schema@1.0.0)"));
    let wrapper = child_of(&host, "wrapper");
    assert!(
        child_of(&wrapper, "client").references.borrow().contains("client@1.0.0(schema@2.0.0)"),
        "the host's client@1.0.0(schema@1.0.0) would shadow a deduped copy",
    );
}

fn make_transitive_peer_client(name: &str, depth: usize) -> Rc<HoisterTree> {
    let mut bridge = tree_node("schema", "schema@2.0.0", &[], vec![]);
    for level in 0..depth {
        let bridge_name = format!("bridge-{level}");
        bridge = tree_node(
            &bridge_name,
            &format!("{bridge_name}@1.0.0(schema@2.0.0)"),
            &["schema"],
            vec![bridge],
        );
    }
    tree_node(name, &format!("{name}@1.0.0(schema@2.0.0)"), &["schema"], vec![bridge])
}
