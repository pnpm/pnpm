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

/// Materialize a member's own package directory — including a
/// `package.json` declaring `sibling_deps` plus a shared `react` — and a
/// `react` dependency dir, inside its slot. `link_declared_siblings`
/// reads that manifest to decide which siblings to link.
fn create_member_slot(slot_modules_dir: &Path, name: &str, sibling_deps: &[&str]) {
    let pkg_dir = slot_modules_dir.join(name);
    fs::create_dir_all(&pkg_dir).unwrap();
    fs::create_dir_all(slot_modules_dir.join("react")).unwrap();

    let mut dependencies = serde_json::Map::new();
    for dep in sibling_deps {
        dependencies.insert((*dep).to_string(), serde_json::json!("workspace:*"));
    }
    dependencies.insert("react".to_string(), serde_json::json!("16"));
    let manifest =
        serde_json::json!({ "name": name, "version": "1.0.0", "dependencies": dependencies });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).unwrap();
}

/// A flagged root importer's injected members each gain symlinks only to
/// the siblings *they declare* — matching pnpm's per-package child
/// linking. The `comp3 → comp2 → comp1` chain (`a → b → c`) resolves
/// transitively through each member's own slot, not an all-to-all clique.
#[test]
fn injected_members_link_declared_siblings() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    // a depends on b; b depends on c; c depends on neither.
    let members: [(&str, &str, &[&str]); 3] = [
        ("@scope/a", "a", &["@scope/b"]),
        ("@scope/b", "b", &["@scope/c"]),
        ("@scope/c", "c", &[]),
    ];
    let slot_dirs: HashMap<&str, std::path::PathBuf> = members
        .iter()
        .map(|(name, payload, deps)| {
            let slot = member_slot_modules_dir(&layout, name, payload);
            create_member_slot(&slot, name, deps);
            (*name, slot)
        })
        .collect();

    let mut deps = ResolvedDependencyMap::new();
    for (name, payload, _) in members {
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

    link_root_component_members(
        &layout,
        &importers,
        &id_set(&[importer_id.as_str()]),
        &[DependencyGroup::Prod],
        &SkippedSnapshots::default(),
    )
    .expect("linking should succeed");

    let a_slot = &slot_dirs["@scope/a"];
    let b_slot = &slot_dirs["@scope/b"];
    let c_slot = &slot_dirs["@scope/c"];

    // A member's own package dir stays a real directory, never a symlink.
    assert!(
        a_slot.join("@scope/a").is_dir()
            && !is_symlink_or_junction(&a_slot.join("@scope/a")).unwrap(),
    );

    // `a` links its declared sibling `b`, but NOT `c` — it doesn't
    // declare `c` directly (that edge lives on `b`).
    assert!(
        is_symlink_or_junction(&a_slot.join("@scope/b")).unwrap(),
        "a must link declared sibling b",
    );
    assert!(!a_slot.join("@scope/c").exists(), "a must not link c — not directly declared");
    assert_eq!(
        fs::canonicalize(a_slot.join("@scope/b")).unwrap(),
        fs::canonicalize(b_slot.join("@scope/b")).unwrap(),
        "a -> b must point at b's package dir",
    );

    // `b` links its declared sibling `c`, but NOT `a`.
    assert!(
        is_symlink_or_junction(&b_slot.join("@scope/c")).unwrap(),
        "b must link declared sibling c",
    );
    assert!(!b_slot.join("@scope/a").exists(), "b must not link a — not declared");
    assert_eq!(
        fs::canonicalize(b_slot.join("@scope/c")).unwrap(),
        fs::canonicalize(c_slot.join("@scope/c")).unwrap(),
        "b -> c must point at c's package dir",
    );

    // `c` declares no siblings and gains none.
    assert!(!c_slot.join("@scope/a").exists());
    assert!(!c_slot.join("@scope/b").exists());

    // The shared `react` dep is never treated as a member.
    for slot in [a_slot, b_slot, c_slot] {
        assert!(!is_symlink_or_junction(&slot.join("react")).unwrap());
    }

    // End-to-end reachability mirrors Node's upward `node_modules` walk:
    // `a` requires `b` (in a's slot), then `b` requires `c` (in b's slot).
    let b_pkg_dir = fs::canonicalize(a_slot.join("@scope/b")).unwrap();
    // `<b_slot>/node_modules/@scope/b` + `../c` → `<b_slot>/node_modules/@scope/c`.
    let c_via_b = fs::canonicalize(b_pkg_dir.join("../c")).unwrap();
    assert_eq!(c_via_b, fs::canonicalize(c_slot.join("@scope/c")).unwrap());

    drop(dir);
}

/// An importer that is NOT flagged as a root component keeps its
/// injected members isolated — the pass never fires for ordinary
/// installs.
#[test]
fn non_root_component_importer_is_untouched() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    let a_slot = member_slot_modules_dir(&layout, "@scope/a", "a");
    let b_slot = member_slot_modules_dir(&layout, "@scope/b", "b");
    // `a` declares `b`, but the importer is not flagged, so nothing links.
    create_member_slot(&a_slot, "@scope/a", &["@scope/b"]);
    create_member_slot(&b_slot, "@scope/b", &[]);

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

/// The linking is purely additive: a sibling a member already resolves
/// for itself (a real dependency symlink) is never clobbered.
#[test]
fn existing_member_dependency_is_not_clobbered() {
    let dir = tempdir().unwrap();
    let layout = VirtualStoreLayout::legacy(dir.path().join("vs"), 120);

    let a_slot = member_slot_modules_dir(&layout, "@scope/a", "a");
    let b_slot = member_slot_modules_dir(&layout, "@scope/b", "b");
    // Both declare each other; `a` already has a pre-existing `@scope/b`.
    create_member_slot(&a_slot, "@scope/a", &["@scope/b"]);
    create_member_slot(&b_slot, "@scope/b", &["@scope/a"]);

    // `a` already resolves `@scope/b` to a pre-existing directory of its
    // own (stand-in for a real, differently-resolved dependency). The
    // pass must leave it alone.
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
    .expect("linking should succeed");

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
        "a missing declared sibling must be filled in",
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
