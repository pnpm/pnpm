use super::{HOISTING_LIMITS_WORKSPACES, injected_member_key, link_root_component_members};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pacquet_lockfile::{
    ImporterDepVersion, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_testing_utils::fs::is_symlink_or_junction;
use std::{collections::HashMap, fs, path::Path};
use tempfile::tempdir;

/// Bit stamps `installConfig.hoistingLimits: "workspaces"` on the
/// per-root importer manifests it generates. Pin the constant the
/// gate keys off so a rename can't silently disable the feature.
#[test]
fn hoisting_limits_workspaces_constant() {
    assert_eq!(HOISTING_LIMITS_WORKSPACES, "workspaces");
}

/// Injected members are detected in both lockfile shapes: the scoped
/// `@scope/name@file:<path>(peers)` form (which parses to `Alias`
/// because it leads with the package name — the shape Bit actually
/// writes) and a bare `file:<path>`. Registry, `link:`, and npm-alias
/// deps are never members.
#[test]
fn injected_member_key_matches_file_and_file_alias() {
    let name: PkgName = "@scope/comp2".parse().unwrap();

    // `@scope/comp2@file:comp2(react@16.14.0)` → `Alias`, matched.
    let alias = ResolvedDependencySpec {
        specifier: "workspace:*".to_string(),
        version: "@scope/comp2@file:comp2(react@16.14.0)".parse().unwrap(),
    };
    assert!(matches!(alias.version, ImporterDepVersion::Alias(_)));
    let (dir_name, key) = injected_member_key(&name, &alias).expect("file alias is a member");
    assert_eq!(dir_name, "@scope/comp2");
    assert_eq!(key.to_string(), "@scope/comp2@file:comp2(react@16.14.0)");

    // Bare `file:comp2` → `File`, matched.
    let file = ResolvedDependencySpec {
        specifier: "workspace:*".to_string(),
        version: ImporterDepVersion::File("comp2".to_string()),
    };
    let (dir_name, key) = injected_member_key(&name, &file).expect("bare file is a member");
    assert_eq!(dir_name, "@scope/comp2");
    assert_eq!(key.to_string(), "@scope/comp2@file:comp2");

    // A registry version and a real npm alias are not members.
    let registry = ResolvedDependencySpec {
        specifier: "^16".to_string(),
        version: "16.14.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
    };
    assert!(injected_member_key(&name, &registry).is_none());
    let npm_alias = ResolvedDependencySpec {
        specifier: "npm:other@1".to_string(),
        version: "other@1.0.0".parse().unwrap(),
    };
    assert!(matches!(npm_alias.version, ImporterDepVersion::Alias(_)));
    assert!(injected_member_key(&name, &npm_alias).is_none());
}

/// Build a `file:`-injected member spec in the scoped
/// `@scope/name@file:<payload>` shape `injectWorkspacePackages`
/// produces for a root component's sibling members (it parses to
/// `Alias`).
fn file_member(name: &str, payload: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: "workspace:*".to_string(),
        version: format!("{name}@file:{payload}").parse().unwrap(),
    }
}

/// The virtual-store slot directory an injected member resolves to,
/// via the same [`VirtualStoreLayout::slot_dir`] path the linker uses.
fn member_slot_modules_dir(
    layout: &VirtualStoreLayout,
    name: &str,
    payload: &str,
) -> std::path::PathBuf {
    let key = PackageKey::new(
        name.parse::<PkgName>().unwrap(),
        format!("file:{payload}").parse().unwrap(),
    );
    layout.slot_dir(&key).join("node_modules")
}

/// Materialize a member's own package directory (the target siblings
/// point at) plus a shared `react` dependency inside its slot, so a
/// resolution walk that lands in the slot has something real to find.
fn create_member_slot(slot_modules_dir: &Path, name: &str) {
    fs::create_dir_all(slot_modules_dir.join(name)).unwrap();
    fs::create_dir_all(slot_modules_dir.join("react")).unwrap();
}

/// A root importer flagged `hoistingLimits: workspaces` gets its
/// injected members cross-linked: every member's slot `node_modules/`
/// gains a symlink to every other member's package directory, so a
/// `realpath`-based walk from one member reaches its siblings.
#[test]
fn injected_members_are_cross_linked() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    // Three members model the `comp3 -> comp2 -> comp1` chain: each
    // sibling must be reachable from every other so the transitive walk
    // resolves.
    let members = [("@scope/a", "a"), ("@scope/b", "b"), ("@scope/c", "c")];
    let slot_dirs: HashMap<&str, std::path::PathBuf> = members
        .iter()
        .map(|(name, payload)| {
            let slot = member_slot_modules_dir(&layout, name, payload);
            create_member_slot(&slot, name);
            (*name, slot)
        })
        .collect();

    let mut deps = ResolvedDependencyMap::new();
    for (name, payload) in members {
        deps.insert(name.parse().unwrap(), file_member(name, payload));
    }
    // A shared registry dep on the root must NOT be treated as a member.
    deps.insert(
        "react".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "16".to_string(),
            version: "16.14.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );

    let importer_id = "node_modules/.bit_roots/scope_comp".to_string();
    let mut importers = HashMap::new();
    importers.insert(
        importer_id.clone(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    let root_component_importers = id_set(&[importer_id.as_str()]);
    link_root_component_members(
        &layout,
        &importers,
        &root_component_importers,
        &[DependencyGroup::Prod],
        &SkippedSnapshots::default(),
    )
    .expect("cross-link should succeed");

    for (host_name, _) in members {
        let host_slot = &slot_dirs[host_name];
        for (sibling_name, _) in members {
            let link = host_slot.join(sibling_name);
            if sibling_name == host_name {
                // A member's own package dir stays a real directory; it
                // is never turned into a self-referential symlink.
                assert!(link.is_dir() && !is_symlink_or_junction(&link).unwrap());
                continue;
            }
            assert!(
                is_symlink_or_junction(&link).unwrap(),
                "expected {host_name} slot to link sibling {sibling_name} at {link:?}",
            );
            let expected = slot_dirs[sibling_name].join(sibling_name);
            assert_eq!(
                fs::canonicalize(&link).unwrap(),
                fs::canonicalize(&expected).unwrap(),
                "{host_name} -> {sibling_name} must point at the sibling package dir",
            );
        }
        // The shared `react` dep is untouched — it is not a member.
        assert!(!is_symlink_or_junction(&host_slot.join("react")).unwrap());
    }

    // End-to-end reachability mirrors Node's upward `node_modules`
    // walk. Resolving `b` from `a`'s package dir follows the cross-link
    // in `a`'s slot to `b`'s package dir; `b`'s sibling `c` then
    // resolves against `b`'s slot `node_modules/` — the parent of `b`'s
    // package dir — where the cross-link into `c` lives.
    let b_pkg_dir = fs::canonicalize(slot_dirs["@scope/a"].join("@scope/b")).unwrap();
    // `<b_slot>/node_modules/@scope/b` + `../c` → `<b_slot>/node_modules/@scope/c`.
    let c_via_b = fs::canonicalize(b_pkg_dir.join("../c")).unwrap();
    assert_eq!(c_via_b, fs::canonicalize(slot_dirs["@scope/c"].join("@scope/c")).unwrap());

    drop(dir);
}

/// An importer that is NOT flagged as a root component keeps its
/// injected members isolated — the cross-link never fires for ordinary
/// installs.
#[test]
fn non_root_component_importer_is_untouched() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    let a_slot = member_slot_modules_dir(&layout, "@scope/a", "a");
    let b_slot = member_slot_modules_dir(&layout, "@scope/b", "b");
    create_member_slot(&a_slot, "@scope/a");
    create_member_slot(&b_slot, "@scope/b");

    let mut deps = ResolvedDependencyMap::new();
    deps.insert("@scope/a".parse().unwrap(), file_member("@scope/a", "a"));
    deps.insert("@scope/b".parse().unwrap(), file_member("@scope/b", "b"));

    let mut importers = HashMap::new();
    importers.insert(
        "packages/app".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    // The flagged set is empty → the importer above is not a root
    // component.
    link_root_component_members(
        &layout,
        &importers,
        &id_set(&[]),
        &[DependencyGroup::Prod],
        &SkippedSnapshots::default(),
    )
    .expect("no-op should succeed");

    assert!(!a_slot.join("@scope/b").exists(), "non-root importer must not gain sibling links");
    assert!(!b_slot.join("@scope/a").exists(), "non-root importer must not gain sibling links");

    drop(dir);
}

/// The cross-link is purely additive: an entry a member already
/// resolves for itself (a real dependency symlink) is never clobbered.
#[test]
fn existing_member_dependency_is_not_clobbered() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    let a_slot = member_slot_modules_dir(&layout, "@scope/a", "a");
    let b_slot = member_slot_modules_dir(&layout, "@scope/b", "b");
    create_member_slot(&a_slot, "@scope/a");
    create_member_slot(&b_slot, "@scope/b");

    // `a` already resolves `@scope/b` to a pre-existing directory of its
    // own (stand-in for a real, differently-resolved dependency). The
    // cross-link must leave it alone.
    let preexisting = dir.path().join("preexisting-b");
    fs::create_dir_all(&preexisting).unwrap();
    fs::create_dir_all(a_slot.join("@scope")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&preexisting, a_slot.join("@scope/b")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&preexisting, a_slot.join("@scope/b")).unwrap();

    let mut deps = ResolvedDependencyMap::new();
    deps.insert("@scope/a".parse().unwrap(), file_member("@scope/a", "a"));
    deps.insert("@scope/b".parse().unwrap(), file_member("@scope/b", "b"));

    let importer_id = "node_modules/.bit_roots/root".to_string();
    let mut importers = HashMap::new();
    importers.insert(
        importer_id.clone(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    link_root_component_members(
        &layout,
        &importers,
        &id_set(&[importer_id.as_str()]),
        &[DependencyGroup::Prod],
        &SkippedSnapshots::default(),
    )
    .expect("cross-link should succeed");

    // `a`'s pre-existing `@scope/b` still points at the original dir.
    assert_eq!(
        fs::canonicalize(a_slot.join("@scope/b")).unwrap(),
        fs::canonicalize(&preexisting).unwrap(),
        "a pre-existing member dependency must not be overwritten",
    );
    // `b`, which had no `@scope/a`, gains the additive sibling link.
    assert_eq!(
        fs::canonicalize(b_slot.join("@scope/a")).unwrap(),
        fs::canonicalize(a_slot.join("@scope/a")).unwrap(),
        "a missing sibling must be filled in",
    );

    drop(dir);
}

/// An empty flagged set short-circuits before touching any importer.
#[test]
fn empty_root_component_set_is_a_no_op() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);
    let importers = HashMap::new();
    let result = link_root_component_members(
        &layout,
        &importers,
        &id_set(&[]),
        &[DependencyGroup::Prod],
        &SkippedSnapshots::default(),
    );
    assert!(result.is_ok());
    drop(dir);
}

/// Build the flagged-importer set the linker gates on, from a slice of
/// importer ids.
fn id_set(ids: &[&str]) -> std::collections::HashSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}
