use super::{
    BTreeSet,
    HashMap,
    HoistOpts,
    HoisterDependencyKind,
    HoisterTree,
    IndexSet,
    Lockfile,
    ProjectSnapshot,
    Rc,
    RcByPtr,
    RefCell,
    SnapshotDepRef,
    SnapshotEntry,
    assert_eq,
    child_of,
    dep_key,
    empty_lockfile,
    hoist,
    nm_hoist,
    pkg_metadata_with_peer,
    pkg_name,
    resolved_dep,
    tree_node,
    tree_root,
    ver_peer,
};

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
fn a_peer_provider_hoisted_above_a_nested_root_lets_copies_share_its_slot() {
    let schema2 = tree_node("schema", "schema@2.0.0", &[], vec![]);
    for client_deps in [vec![], vec![Rc::clone(&schema2)]] {
        let client2 = tree_node("client", "client@2.0.0(schema@2.0.0)", &["schema"], client_deps);
        let wrapper_a2 = tree_node("wrapper-a", "wrapper-a@2.0.0", &[], vec![Rc::clone(&client2)]);
        let wrapper_b2 = tree_node("wrapper-b", "wrapper-b@2.0.0", &[], vec![client2]);
        let host =
            tree_node("host", "host@1.0.0", &[], vec![wrapper_a2, wrapper_b2, Rc::clone(&schema2)]);
        let root = tree_root(vec![
            tree_node("client", "client@1.0.0", &[], vec![]),
            Rc::clone(&schema2),
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
        assert!(
            child_of(&host, "client").references.borrow().contains("client@2.0.0(schema@2.0.0)"),
        );
    }
}

/// `a` is a hoist border that holds `b`, and `b` depends on another peer
/// variant of `a`. Hoisting that variant into `a` drops it, since `a`
/// serves it by name, and `a` lists the variant's key with its own.
#[test]
fn a_root_that_absorbs_its_own_package_serves_its_snapshot_key() {
    let inner = tree_node("a", "a@1.0.0(p@2.0.0)", &[], vec![]);
    let holder = tree_node("b", "b@1.0.0", &[], vec![inner]);
    let border = tree_node("a", "a@1.0.0(p@1.0.0)", &[], vec![holder]);
    let root = tree_root(vec![border]);
    let mut opts = HoistOpts::default();
    opts.hoisting_limits.insert(".@".to_string(), BTreeSet::from(["a".to_string()]));

    let result = nm_hoist(&root, &opts);

    let border = child_of(&result, "a");
    let served: Vec<String> = border.references
        .borrow()
        .iter()
        .cloned()
        .collect();
    assert_eq!(served, ["a@1.0.0(p@1.0.0)", "a@1.0.0(p@2.0.0)"], "{result:#?}");
    assert!(child_of(&border, "b").dependencies.borrow().is_empty(), "b finds a above it");
}

/// `n` takes its sibling `s` as a peer and depends on `q@2`, which the root
/// cannot hold next to `q@1` and which moves up to `p`. Once `s` can move
/// to the root, `n` may not follow it: from the root it would find `q@1`.
#[test]
fn a_node_waiting_on_a_peer_provider_stays_when_its_dependency_would_change() {
    let soft_link = Rc::new(HoisterTree {
        identity: None,
        name: "s".to_string(),
        ident_name: "s".to_string(),
        reference: "link:s".to_string(),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::ExternalSoftLink,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::from([RcByPtr(tree_node(
            "d",
            "d@1.0.0",
            &[],
            vec![],
        ))])),
    });
    let conflicting = tree_node("q", "q@2.0.0", &[], vec![]);
    let dependent = tree_node("n", "n@1.0.0", &["s"], vec![conflicting, Rc::clone(&soft_link)]);
    let parent = tree_node("p", "p@1.0.0", &[], vec![dependent, soft_link]);
    let root = tree_root(vec![tree_node("q", "q@1.0.0", &[], vec![]), parent]);

    let result = nm_hoist(&root, &HoistOpts::default());

    let parent = child_of(&result, "p");
    assert!(child_of(&parent, "q").references.borrow().contains("q@2.0.0"), "{result:#?}");
    child_of(&parent, "n");
    child_of(&result, "s");
}
