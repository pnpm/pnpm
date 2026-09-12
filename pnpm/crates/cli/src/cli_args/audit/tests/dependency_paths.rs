use super::{
    BTreeMap, EnvLockfile, Include, MAX_PATHS_PER_FINDING, SnapshotEntry, SpecifierAndResolution,
    all_dependencies, build_audit_path_index, empty_lockfile, fixture_env_lockfile,
    fixture_lockfile, lockfile_to_audit_request, parse_lockfile, path_info, prod_without_optional,
    snapshot, vulnerable_names,
};
use std::fmt::Write as _;

#[test]
fn lockfile_to_audit_request_includes_project_and_env_dependencies() {
    let lockfile = fixture_lockfile();
    let env_lockfile = fixture_env_lockfile();
    let request = lockfile_to_audit_request(&lockfile, Some(&env_lockfile), all_dependencies());

    assert_eq!(request.request["prod"], vec!["1.0.0"]);
    assert_eq!(request.request["transitive"], vec!["2.0.0"]);
    assert_eq!(request.request["transitive-optional"], vec!["3.0.0"]);
    assert_eq!(request.request["dev-only"], vec!["1.0.0"]);
    assert_eq!(request.request["optional-only"], vec!["1.0.0"]);
    assert_eq!(request.request["config-dep"], vec!["1.0.0"]);
    assert_eq!(request.total_dependencies, 6);
    assert_eq!(request.dependencies, 3);
    assert_eq!(request.dev_dependencies, 1);
    assert_eq!(request.optional_dependencies, 2);
}

#[test]
fn lockfile_to_audit_request_respects_prod_and_no_optional() {
    let lockfile = fixture_lockfile();
    let env_lockfile = fixture_env_lockfile();
    let request =
        lockfile_to_audit_request(&lockfile, Some(&env_lockfile), prod_without_optional());

    assert_eq!(request.request["prod"], vec!["1.0.0"]);
    assert_eq!(request.request["transitive"], vec!["2.0.0"]);
    assert_eq!(request.request["config-dep"], vec!["1.0.0"]);
    assert!(!request.request.contains_key("dev-only"));
    assert!(!request.request.contains_key("optional-only"));
    assert!(!request.request.contains_key("transitive-optional"));
    assert_eq!(request.total_dependencies, 3);
    assert_eq!(request.dependencies, 3);
    assert_eq!(request.dev_dependencies, 0);
    assert_eq!(request.optional_dependencies, 0);
}

#[test]
fn lockfile_to_audit_request_accepts_absent_env_lockfile() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  foo@1.0.0: {}
",
    );

    let request = lockfile_to_audit_request(&lockfile, None, all_dependencies());

    assert_eq!(request.request["foo"], vec!["1.0.0"]);
    assert_eq!(request.total_dependencies, 1);
}

#[test]
fn lockfile_to_audit_request_includes_env_package_manager_dependencies() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  foo@1.0.0: {}
",
    );
    let mut env = EnvLockfile::create();
    {
        let importer = env.root_importer_mut();
        importer.config_dependencies.insert(
            "my-config".to_string(),
            SpecifierAndResolution { specifier: "2.0.0".to_string(), version: "2.0.0".to_string() },
        );
        importer.package_manager_dependencies = Some(BTreeMap::from([(
            "pnpm".to_string(),
            SpecifierAndResolution { specifier: "9.0.0".to_string(), version: "9.0.0".to_string() },
        )]));
    }
    env.snapshots
        .insert("my-config@2.0.0".parse().unwrap(), snapshot(&[("config-util", "1.0.0")], &[]));
    env.snapshots.insert("config-util@1.0.0".parse().unwrap(), SnapshotEntry::default());
    env.snapshots.insert("pnpm@9.0.0".parse().unwrap(), SnapshotEntry::default());

    let request = lockfile_to_audit_request(&lockfile, Some(&env), all_dependencies());

    assert_eq!(request.request["foo"], vec!["1.0.0"]);
    assert_eq!(request.request["my-config"], vec!["2.0.0"]);
    assert_eq!(request.request["config-util"], vec!["1.0.0"]);
    assert_eq!(request.request["pnpm"], vec!["9.0.0"]);
}

#[test]
fn lockfile_to_audit_request_includes_optional_dependencies_from_env_snapshots() {
    let lockfile = empty_lockfile();
    let mut env = EnvLockfile::create();
    env.root_importer_mut().config_dependencies.insert(
        "my-tool".to_string(),
        SpecifierAndResolution { specifier: "1.0.0".to_string(), version: "1.0.0".to_string() },
    );
    env.snapshots.insert(
        "my-tool@1.0.0".parse().unwrap(),
        snapshot(&[("required-dep", "1.0.0")], &[("optional-dep", "2.0.0")]),
    );
    env.snapshots.insert("required-dep@1.0.0".parse().unwrap(), SnapshotEntry::default());
    env.snapshots.insert("optional-dep@2.0.0".parse().unwrap(), SnapshotEntry::default());

    let request = lockfile_to_audit_request(&lockfile, Some(&env), all_dependencies());

    assert_eq!(request.request["required-dep"], vec!["1.0.0"]);
    assert_eq!(request.request["optional-dep"], vec!["2.0.0"]);

    let without_optional =
        lockfile_to_audit_request(&lockfile, Some(&env), prod_without_optional());
    assert_eq!(without_optional.request["required-dep"], vec!["1.0.0"]);
    assert!(!without_optional.request.contains_key("optional-dep"));
}

#[test]
fn lockfile_to_audit_request_ignores_unreachable_env_packages() {
    let lockfile = empty_lockfile();
    let mut env = EnvLockfile::create();
    env.root_importer_mut().config_dependencies.insert(
        "my-config".to_string(),
        SpecifierAndResolution { specifier: "1.0.0".to_string(), version: "1.0.0".to_string() },
    );
    env.snapshots.insert("my-config@1.0.0".parse().unwrap(), SnapshotEntry::default());
    env.snapshots.insert("orphan-pkg@3.0.0".parse().unwrap(), SnapshotEntry::default());

    let request = lockfile_to_audit_request(&lockfile, Some(&env), all_dependencies());

    assert!(request.request.contains_key("my-config"));
    assert!(!request.request.contains_key("orphan-pkg"));
}

#[test]
fn build_audit_path_index_records_install_paths_for_vulnerable_packages() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  foo@1.0.0:
    dependencies:
      bar: '1.0.0'

  bar@1.0.0: {}
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["bar"]), all_dependencies());

    let info = path_info(&index, "bar", "1.0.0");
    assert_eq!(info.paths, vec![".>foo>bar"]);
    assert!(!info.dev);
    assert!(!info.optional);
    assert!(!index.contains_key("foo"));
}

#[test]
fn build_audit_path_index_records_every_distinct_install_path_for_shared_dependencies() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      a:
        specifier: '1.0.0'
        version: '1.0.0'
      b:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  a@1.0.0:
    dependencies:
      lodash: '4.0.0'

  b@1.0.0:
    dependencies:
      lodash: '4.0.0'

  lodash@4.0.0: {}
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["lodash"]), all_dependencies());

    let mut paths = path_info(&index, "lodash", "4.0.0").paths.clone();
    paths.sort();
    assert_eq!(paths, vec![".>a>lodash", ".>b>lodash"]);
}

#[test]
fn build_audit_path_index_keeps_all_vulnerable_paths_while_ignoring_non_vulnerable_nodes() {
    let mut importers = String::new();
    let mut snapshots = String::from(
        "
  vuln@1.0.0: {}

  cold@1.0.0:
    dependencies:
      cold-leaf: '1.0.0'

  cold-leaf@1.0.0: {}
",
    );
    for i in 0..50 {
        write!(
            importers,
            "
  .{i}:
    dependencies:
      parent-{i}:
        specifier: '1.0.0'
        version: '1.0.0'
",
        )
        .unwrap();
        write!(
            snapshots,
            "
  parent-{i}@1.0.0:
    dependencies:
      cold: '1.0.0'
      vuln: '1.0.0'
",
        )
        .unwrap();
    }
    let lockfile = parse_lockfile(&format!(
        "
lockfileVersion: '9.0'

importers:
{importers}
snapshots:
{snapshots}
",
    ));

    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["vuln"]), all_dependencies());

    assert_eq!(path_info(&index, "vuln", "1.0.0").paths.len(), 50);
    assert!(!index.contains_key("cold"));
    assert!(!index.contains_key("cold-leaf"));
}

#[test]
fn build_audit_path_index_limits_paths_per_finding() {
    let mut importers = String::new();
    for i in 0..150 {
        write!(
            importers,
            "
  .{i}:
    dependencies:
      vuln:
        specifier: '1.0.0'
        version: '1.0.0'
",
        )
        .unwrap();
    }
    let lockfile = parse_lockfile(&format!(
        "
lockfileVersion: '9.0'

importers:
{importers}
snapshots:

  vuln@1.0.0: {{}}
",
    ));

    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["vuln"]), all_dependencies());

    assert_eq!(path_info(&index, "vuln", "1.0.0").paths.len(), MAX_PATHS_PER_FINDING);
}

#[test]
fn build_audit_path_index_classifies_optional_when_only_included_path_is_optional() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    devDependencies:
      dev-root:
        specifier: '1.0.0'
        version: '1.0.0'
    optionalDependencies:
      opt-root:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  dev-root@1.0.0:
    dependencies:
      shared-pkg: '1.0.0'

  opt-root@1.0.0:
    dependencies:
      shared-pkg: '1.0.0'

  shared-pkg@1.0.0: {}
",
    );

    let with_dev = build_audit_path_index(
        &lockfile,
        None,
        &vulnerable_names(&["shared-pkg"]),
        all_dependencies(),
    );
    assert!(!path_info(&with_dev, "shared-pkg", "1.0.0").optional);

    let prod_only = build_audit_path_index(
        &lockfile,
        None,
        &vulnerable_names(&["shared-pkg"]),
        Include { dependencies: true, dev_dependencies: false, optional_dependencies: true },
    );
    assert!(path_info(&prod_only, "shared-pkg", "1.0.0").optional);
}

#[test]
fn build_audit_path_index_flags_findings_reached_only_through_optional_edges() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    optionalDependencies:
      native:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  native@1.0.0: {}
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["native"]), all_dependencies());

    let info = path_info(&index, "native", "1.0.0");
    assert_eq!(info.paths, vec![".>native"]);
    assert!(info.optional);
    assert!(!info.dev);
}

#[test]
fn build_audit_path_index_preserves_reachability_across_cycles() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      a:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  a@1.0.0:
    dependencies:
      b: '1.0.0'

  b@1.0.0:
    dependencies:
      a: '1.0.0'
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["a", "b"]), all_dependencies());

    assert_eq!(path_info(&index, "a", "1.0.0").paths, vec![".>a"]);
    assert_eq!(path_info(&index, "b", "1.0.0").paths, vec![".>a>b"]);
}

#[test]
fn build_audit_path_index_preserves_reachability_when_cycle_root_is_queried_later() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      root:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  root@1.0.0:
    dependencies:
      b: '1.0.0'

  a@1.0.0:
    dependencies:
      b: '1.0.0'

  b@1.0.0:
    dependencies:
      a: '1.0.0'
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["a"]), all_dependencies());

    assert_eq!(path_info(&index, "a", "1.0.0").paths, vec![".>root>b>a"]);
}

#[test]
fn build_audit_path_index_keeps_paths_reached_through_non_entry_cycle_member() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      c:
        specifier: '1.0.0'
        version: '1.0.0'
      b:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  b@1.0.0:
    dependencies:
      c: '1.0.0'

  c@1.0.0:
    dependencies:
      b: '1.0.0'
      x: '1.0.0'

  x@1.0.0: {}
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["x"]), all_dependencies());

    let mut paths = path_info(&index, "x", "1.0.0").paths.clone();
    paths.sort();
    assert_eq!(paths, vec![".>b>c>x", ".>c>x"]);
}

#[test]
fn build_audit_path_index_handles_large_cycle_with_vulnerable_leaf() {
    let size = 400;
    let mut snapshots = String::new();
    for i in 0..size {
        let next = if i + 1 < size { format!("n{}", i + 1) } else { "n0".to_string() };
        write!(
            snapshots,
            "
  n{i}@1.0.0:
    dependencies:
      {next}: '1.0.0'
      leaf{i}: '1.0.0'

  leaf{i}@1.0.0: {{}}
",
        )
        .unwrap();
    }
    let lockfile = parse_lockfile(&format!(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      n0:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:
{snapshots}
",
    ));
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["leaf0"]), all_dependencies());

    let info = path_info(&index, "leaf0", "1.0.0");
    assert_eq!(info.paths, vec![".>n0>leaf0"]);
}

#[test]
fn build_audit_path_index_handles_very_deep_dependency_chain() {
    let size = 12_000;
    let mut snapshots = String::new();
    for i in 0..size {
        let child = if i + 1 < size { format!("n{}", i + 1) } else { "vuln".to_string() };
        write!(
            snapshots,
            "
  n{i}@1.0.0:
    dependencies:
      {child}: '1.0.0'
",
        )
        .unwrap();
    }
    snapshots.push_str(
        "
  vuln@1.0.0: {}
",
    );
    let lockfile = parse_lockfile(&format!(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      n0:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:
{snapshots}
",
    ));

    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["vuln"]), all_dependencies());

    let info = path_info(&index, "vuln", "1.0.0");
    assert_eq!(info.paths.len(), 1);
    assert!(info.paths[0].starts_with(".>n0>n1>"));
    assert!(info.paths[0].ends_with(">vuln"));
}

#[test]
fn build_audit_path_index_replaces_slashes_in_workspace_importer_ids() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  packages/foo:
    dependencies:
      foo:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  foo@1.0.0: {}
",
    );
    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["foo"]), all_dependencies());

    assert_eq!(path_info(&index, "foo", "1.0.0").paths, vec!["packages__foo>foo"]);
}
