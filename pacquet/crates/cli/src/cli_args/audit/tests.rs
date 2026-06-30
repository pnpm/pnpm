use super::{
    AuditAdvisory, AuditFinding, AuditMetadata, AuditPathIndex, AuditReport,
    AuditVulnerabilityCounts, BTreeMap, Config, ConfigAuditLevel, Cwe, HashMap, Include,
    InstalledPackages, MAX_PATHS_PER_FINDING, PackageVersionGuard, PackageVersionGuardDecision,
    PathInfo, Range, RawBulkAdvisory, SnapshotDepRef, VulnerabilityGuard, build_audit_path_index,
    bulk_response_to_audit_report, caret_range_for_patched, classify_for_update, create_overrides,
    filter_advisories_for_fix, filter_ignored_advisories, format_fix_with_update_output,
    lockfile_to_audit_request, minimum_release_age_excludes, normalize_ghsa_id,
    redact_url_userinfo, render_json_report, render_text_report, report_fixed_remaining,
    sanitize_control_chars, satisfies_safe,
};
use pacquet_lockfile::{EnvLockfile, Lockfile, SnapshotEntry, SpecifierAndResolution};
use std::{collections::HashSet, fmt::Write as _};

fn parse_lockfile(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse lockfile fixture")
}

fn fixture_lockfile() -> Lockfile {
    parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      prod:
        specifier: '1.0.0'
        version: '1.0.0'
    devDependencies:
      dev-only:
        specifier: '1.0.0'
        version: '1.0.0'
    optionalDependencies:
      optional-only:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  prod@1.0.0:
    dependencies:
      transitive: '2.0.0'
    optionalDependencies:
      transitive-optional: '3.0.0'

  dev-only@1.0.0: {}

  optional-only@1.0.0: {}

  transitive@2.0.0: {}

  transitive-optional@3.0.0: {}
",
    )
}

fn fixture_env_lockfile() -> EnvLockfile {
    let mut env = EnvLockfile::create();
    env.root_importer_mut().config_dependencies.insert(
        "config-dep".to_string(),
        SpecifierAndResolution { specifier: "1.0.0".to_string(), version: "1.0.0".to_string() },
    );
    env.snapshots.insert("config-dep@1.0.0".parse().unwrap(), SnapshotEntry::default());
    env
}

fn all_dependencies() -> Include {
    Include { dependencies: true, dev_dependencies: true, optional_dependencies: true }
}

fn prod_without_optional() -> Include {
    Include { dependencies: true, dev_dependencies: false, optional_dependencies: false }
}

fn empty_lockfile() -> Lockfile {
    parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .: {}
",
    )
}

fn vulnerable_names(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn path_info<'a>(index: &'a AuditPathIndex, name: &str, version: &str) -> &'a PathInfo {
    index.get(name).and_then(|by_version| by_version.get(version)).expect("path info")
}

fn snapshot(deps: &[(&str, &str)], optional_deps: &[(&str, &str)]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: (!deps.is_empty()).then(|| {
            deps.iter()
                .map(|(name, version)| {
                    ((*name).parse().unwrap(), (*version).parse::<SnapshotDepRef>().unwrap())
                })
                .collect()
        }),
        optional_dependencies: (!optional_deps.is_empty()).then(|| {
            optional_deps
                .iter()
                .map(|(name, version)| {
                    ((*name).parse().unwrap(), (*version).parse::<SnapshotDepRef>().unwrap())
                })
                .collect()
        }),
        ..SnapshotEntry::default()
    }
}

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

#[test]
fn bulk_response_to_audit_report_keeps_only_installed_vulnerable_versions() {
    let lockfile = fixture_lockfile();
    let env_lockfile = fixture_env_lockfile();
    let audit_request =
        lockfile_to_audit_request(&lockfile, Some(&env_lockfile), all_dependencies());
    let mut bulk = BTreeMap::new();
    bulk.insert(
        "transitive".to_string(),
        vec![
            RawBulkAdvisory {
                id: Some(serde_json::json!(101)),
                url: Some("https://github.com/advisories/GHSA-AbCd-1111-2222".to_string()),
                title: Some("transitive issue".to_string()),
                severity: Some("high".to_string()),
                vulnerable_versions: "<3.0.0".to_string(),
                cwe: Some(Cwe::Many(vec!["CWE-1".to_string(), "CWE-2".to_string()])),
            },
            RawBulkAdvisory {
                id: Some(serde_json::json!(102)),
                url: None,
                title: Some("not installed".to_string()),
                severity: Some("critical".to_string()),
                vulnerable_versions: "<2.0.0".to_string(),
                cwe: None,
            },
        ],
    );

    let report = bulk_response_to_audit_report(
        bulk,
        &audit_request,
        &lockfile,
        Some(&env_lockfile),
        all_dependencies(),
    );

    assert_eq!(report.advisories.len(), 1);
    let advisory = &report.advisories["101"];
    assert_eq!(advisory.github_advisory_id, "GHSA-abcd-1111-2222");
    assert_eq!(advisory.patched_versions.as_deref(), Some(">=3.0.0"));
    assert_eq!(advisory.cwe, "CWE-1, CWE-2");
    assert_eq!(advisory.findings.len(), 1);
    assert_eq!(advisory.findings[0].version, "2.0.0");
    assert_eq!(advisory.findings[0].paths, vec![".>prod>transitive"]);
    assert!(!advisory.findings[0].dev);
    assert!(!advisory.findings[0].optional);
    assert_eq!(report.metadata.vulnerabilities.high, 1);
    assert_eq!(report.metadata.vulnerabilities.critical, 0);
}

#[test]
fn bulk_response_to_audit_report_computes_findings_and_counts_from_bare_bulk_response() {
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
    let audit_request = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    let bulk = BTreeMap::from([(
        "bar".to_string(),
        vec![RawBulkAdvisory {
            id: Some(serde_json::json!(42)),
            url: Some("https://github.com/advisories/GHSA-xxxx-yyyy-zzzz".to_string()),
            title: Some("bar is bad".to_string()),
            severity: Some("high".to_string()),
            vulnerable_versions: "<2.0.0".to_string(),
            cwe: None,
        }],
    )]);

    let report =
        bulk_response_to_audit_report(bulk, &audit_request, &lockfile, None, all_dependencies());

    let advisory = &report.advisories["42"];
    assert_eq!(advisory.module_name, "bar");
    assert_eq!(advisory.github_advisory_id, "GHSA-xxxx-yyyy-zzzz");
    assert_eq!(advisory.patched_versions.as_deref(), Some(">=2.0.0"));
    assert_eq!(advisory.findings[0].version, "1.0.0");
    assert_eq!(advisory.findings[0].paths, vec![".>foo>bar"]);
    assert_eq!(report.metadata.vulnerabilities.high, 1);
    assert_eq!(report.metadata.total_dependencies, 2);
}

#[test]
fn bulk_response_to_audit_report_handles_info_severity() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      info-pkg:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  info-pkg@1.0.0: {}
",
    );
    let audit_request = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    let bulk = BTreeMap::from([(
        "info-pkg".to_string(),
        vec![RawBulkAdvisory {
            id: Some(serde_json::json!(100)),
            url: Some("https://github.com/advisories/GHSA-info-info-info".to_string()),
            title: Some("just some info".to_string()),
            severity: Some("info".to_string()),
            vulnerable_versions: "*".to_string(),
            cwe: None,
        }],
    )]);

    let report =
        bulk_response_to_audit_report(bulk, &audit_request, &lockfile, None, all_dependencies());

    assert_eq!(report.metadata.vulnerabilities.info, 1);
    assert_eq!(report.advisories["100"].severity, ConfigAuditLevel::Info);
}

#[test]
fn render_json_filters_by_audit_level_after_ignores() {
    let mut report = AuditReport {
        advisories: BTreeMap::from([
            (
                "1".to_string(),
                advisory(1, "low issue", ConfigAuditLevel::Low, "GHSA-low1-1111-2222"),
            ),
            (
                "2".to_string(),
                advisory(2, "high issue", ConfigAuditLevel::High, "GHSA-high-3333-4444"),
            ),
        ]),
        metadata: AuditMetadata {
            vulnerabilities: AuditVulnerabilityCounts {
                info: 0,
                low: 1,
                moderate: 0,
                high: 1,
                critical: 0,
            },
            dependencies: 2,
            dev_dependencies: 0,
            optional_dependencies: 0,
            total_dependencies: 2,
        },
    };
    let mut config = Config::default();
    config.audit_config.ignore_ghsas = vec!["ghsa-HIGH-3333-4444".to_string()];

    let ignored = filter_ignored_advisories(&mut report, &config);
    assert_eq!(ignored.high, 1);
    assert!(report.advisories.contains_key("1"));
    assert!(!report.advisories.contains_key("2"));

    let rendered = render_json_report(&report, ConfigAuditLevel::Moderate).unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(value["advisories"].as_object().unwrap().len(), 0);
    assert_eq!(value["metadata"]["vulnerabilities"]["low"], 1);
    assert_eq!(value["metadata"]["vulnerabilities"]["high"], 1);
}

#[test]
fn filter_ignored_advisories_does_not_match_blank_ghsa_ids() {
    let mut report = AuditReport {
        advisories: BTreeMap::from([
            ("1".to_string(), advisory(1, "missing ghsa", ConfigAuditLevel::High, "")),
            (
                "2".to_string(),
                advisory(2, "ignored ghsa", ConfigAuditLevel::High, "GHSA-high-3333-4444"),
            ),
        ]),
        metadata: AuditMetadata {
            vulnerabilities: AuditVulnerabilityCounts {
                info: 0,
                low: 0,
                moderate: 0,
                high: 2,
                critical: 0,
            },
            dependencies: 2,
            dev_dependencies: 0,
            optional_dependencies: 0,
            total_dependencies: 2,
        },
    };
    let mut config = Config::default();
    config.audit_config.ignore_ghsas =
        vec![String::new(), "  ".to_string(), "ghsa-HIGH-3333-4444".to_string()];

    let ignored = filter_ignored_advisories(&mut report, &config);

    assert_eq!(ignored.high, 1);
    assert!(report.advisories.contains_key("1"));
    assert!(!report.advisories.contains_key("2"));
}

#[test]
fn satisfies_safe_includes_prerelease_versions_for_audit_ranges() {
    assert!(satisfies_safe("1.2.3-rc.0", "<2.0.0"));
    assert!(satisfies_safe("2.0.0-rc.1", "<2.0.0"));
    assert!(satisfies_safe("1.2.3-beta.0", "^1.2.0"));
    assert!(!satisfies_safe("1.2.0-beta.0", "^1.2.0"));
}

#[test]
fn text_report_separates_advisory_table_from_summary() {
    let report = AuditReport {
        advisories: BTreeMap::from([(
            "1".to_string(),
            advisory(1, "high issue", ConfigAuditLevel::High, "GHSA-high-3333-4444"),
        )]),
        metadata: AuditMetadata {
            vulnerabilities: AuditVulnerabilityCounts {
                info: 0,
                low: 0,
                moderate: 0,
                high: 1,
                critical: 0,
            },
            dependencies: 1,
            dev_dependencies: 0,
            optional_dependencies: 0,
            total_dependencies: 1,
        },
    };

    let output =
        render_text_report(&report, ConfigAuditLevel::Low, 1, &AuditVulnerabilityCounts::default());
    let summary_start = output.find("1 vulnerabilities found").unwrap();
    assert_eq!(output.as_bytes()[summary_start - 1], b'\n');
}

#[test]
fn redact_url_userinfo_removes_credentials_from_audit_endpoint() {
    assert_eq!(
        redact_url_userinfo(
            "https://user:secret@registry.example.com/npm/-/npm/v1/security/advisories/bulk"
        ),
        "https://registry.example.com/npm/-/npm/v1/security/advisories/bulk",
    );
    assert_eq!(
        redact_url_userinfo("https://user@registry.example.com/-/npm/v1/security/advisories/bulk"),
        "https://registry.example.com/-/npm/v1/security/advisories/bulk",
    );
    assert_eq!(redact_url_userinfo("not a url"), "not a url");
}

#[test]
fn sanitize_control_chars_escapes_registry_control_characters() {
    assert_eq!(sanitize_control_chars("ok\u{1b}[31m\n\u{7f}"), r"ok\u{1b}[31m\u{a}\u{7f}");
}

fn advisory(id: u64, title: &str, severity: ConfigAuditLevel, ghsa: &str) -> AuditAdvisory {
    AuditAdvisory {
        findings: vec![AuditFinding {
            version: "1.0.0".to_string(),
            paths: vec![".>pkg".to_string()],
            dev: false,
            optional: false,
            bundled: false,
        }],
        id,
        title: title.to_string(),
        module_name: "pkg".to_string(),
        vulnerable_versions: "<2.0.0".to_string(),
        patched_versions: Some(">=2.0.0".to_string()),
        severity,
        cwe: String::new(),
        github_advisory_id: normalize_ghsa_id(ghsa),
        url: format!("https://github.com/advisories/{ghsa}"),
    }
}

#[test]
fn caret_range_for_patched_uses_minimum_with_caret() {
    assert_eq!(caret_range_for_patched(">=2.0.0"), "^2.0.0");
    assert_eq!(caret_range_for_patched(">=1.2.3"), "^1.2.3");
    // A non-inferred range is passed through unchanged.
    assert_eq!(caret_range_for_patched("not-a-range"), "not-a-range");
}

fn fix_advisory(
    id: u64,
    module_name: &str,
    vulnerable: &str,
    patched: Option<&str>,
    severity: ConfigAuditLevel,
    ghsa: &str,
) -> AuditAdvisory {
    let mut advisory = advisory(id, "title", severity, ghsa);
    advisory.module_name = module_name.to_string();
    advisory.vulnerable_versions = vulnerable.to_string();
    advisory.patched_versions = patched.map(ToString::to_string);
    advisory
}

fn report_of(advisories: Vec<AuditAdvisory>) -> AuditReport {
    AuditReport {
        advisories: advisories
            .into_iter()
            .map(|advisory| (advisory.id.to_string(), advisory))
            .collect(),
        metadata: AuditMetadata {
            vulnerabilities: AuditVulnerabilityCounts::default(),
            dependencies: 0,
            dev_dependencies: 0,
            optional_dependencies: 0,
            total_dependencies: 0,
        },
    }
}

#[test]
fn create_overrides_sorts_and_skips_unfixable() {
    let advisories = BTreeMap::from([
        (
            "1".to_string(),
            fix_advisory(1, "zoo", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        ),
        (
            "2".to_string(),
            fix_advisory(2, "abc", "<1.5.0", Some(">=1.5.0"), ConfigAuditLevel::Low, "GHSA-b"),
        ),
        // No patched range: cannot produce an override.
        (
            "3".to_string(),
            fix_advisory(3, "unfixable", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-c"),
        ),
    ]);

    let overrides = create_overrides(&advisories);

    assert_eq!(
        overrides.into_iter().collect::<Vec<_>>(),
        vec![
            ("abc@<1.5.0".to_string(), "^1.5.0".to_string()),
            ("zoo@<2.0.0".to_string(), "^2.0.0".to_string()),
        ],
    );
}

#[test]
fn filter_advisories_for_fix_drops_below_level_and_ignored() {
    let report = report_of(vec![
        fix_advisory(1, "high", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-high-1"),
        fix_advisory(2, "info", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::Info, "GHSA-info-1"),
        fix_advisory(
            3,
            "ignored",
            "<2.0.0",
            Some(">=2.0.0"),
            ConfigAuditLevel::Critical,
            "GHSA-skip-1",
        ),
    ]);
    let mut config = Config::default();
    config.audit_config.ignore_ghsas = vec!["GHSA-skip-1".to_string()];

    let filtered = filter_advisories_for_fix(&report, ConfigAuditLevel::Low, &config);

    assert!(filtered.contains_key("1"), "high stays");
    assert!(!filtered.contains_key("2"), "info is below the low audit level");
    assert!(!filtered.contains_key("3"), "ignored GHSA is dropped");
}

#[tokio::test]
async fn vulnerability_guard_rejects_only_vulnerable_versions() {
    let guard = VulnerabilityGuard {
        ranges_by_name: HashMap::from([(
            "vulnerable".to_string(),
            vec!["<2.0.0".parse().expect("range")],
        )]),
    };

    let rejected = guard.check("vulnerable", "1.5.0").await.expect("guard check");
    assert!(matches!(rejected, PackageVersionGuardDecision::Reject { .. }));

    let allowed_safe = guard.check("vulnerable", "2.0.0").await.expect("guard check");
    assert_eq!(allowed_safe, PackageVersionGuardDecision::Allow);

    let allowed_other = guard.check("unrelated", "1.0.0").await.expect("guard check");
    assert_eq!(allowed_other, PackageVersionGuardDecision::Allow);
}

#[test]
fn format_fix_with_update_output_lists_fixed_and_remaining() {
    let advisories = BTreeMap::from([
        (
            "1".to_string(),
            fix_advisory(
                1,
                "fixed-pkg",
                "<2.0.0",
                Some(">=2.0.0"),
                ConfigAuditLevel::High,
                "GHSA-a",
            ),
        ),
        (
            "2".to_string(),
            fix_advisory(
                2,
                "stuck-pkg",
                "<2.0.0",
                Some(">=2.0.0"),
                ConfigAuditLevel::Low,
                "GHSA-b",
            ),
        ),
    ]);

    let output = format_fix_with_update_output(&[1], &[2], &advisories);

    assert!(output.contains("1 vulnerability was fixed, 1 vulnerability remains."));
    assert!(output.contains("The fixed vulnerabilities are:"));
    assert!(output.contains("fixed-pkg"));
    assert!(output.contains("The remaining vulnerabilities are:"));
    assert!(output.contains("stuck-pkg"));
}

#[test]
fn minimum_release_age_excludes_uses_patched_minimums_and_skips_unfixable() {
    let advisories = report_of(vec![
        fix_advisory(1, "foo", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        // No patched range: contributes no exclude entry.
        fix_advisory(2, "bar", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-b"),
    ])
    .advisories;

    let excludes = minimum_release_age_excludes(&advisories).expect("compute excludes");

    assert_eq!(excludes, vec!["foo@2.0.0".to_string()]);
}

#[test]
fn classify_for_update_routes_unparsable_ranges_to_remaining() {
    let advisories = report_of(vec![
        fix_advisory(1, "ok", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        fix_advisory(2, "any", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-b"),
        // An untrusted registry could send a range we can't parse.
        fix_advisory(3, "broken", "not a range", None, ConfigAuditLevel::High, "GHSA-c"),
        // ...or pad an unfixable sentinel with whitespace.
        fix_advisory(4, "padded", "  >=0.0.0  ", None, ConfigAuditLevel::High, "GHSA-d"),
    ])
    .advisories;

    let classification = classify_for_update(&advisories);

    assert!(classification.vulnerabilities.contains_key("ok"));
    assert!(classification.unfixable.contains_key("any"));
    assert!(
        classification.unfixable.contains_key("padded"),
        "a padded sentinel is still unfixable",
    );
    assert_eq!(classification.unparsable, vec![3], "an unparsable range must not be dropped");
}

#[test]
fn classify_for_update_trims_module_names() {
    // A whitespace-padded module name from an untrusted registry must key the
    // guard and the installed-name comparison by the clean package name.
    let advisories = report_of(vec![fix_advisory(
        1,
        "  vulnerable  ",
        "<2.0.0",
        Some(">=2.0.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;

    let classification = classify_for_update(&advisories);

    assert!(classification.vulnerabilities.contains_key("vulnerable"));
    assert!(!classification.vulnerabilities.contains_key("  vulnerable  "));
}

#[test]
fn report_fixed_remaining_keeps_non_semver_installed_packages_remaining() {
    let mut vulnerabilities: HashMap<String, Vec<(u64, Range)>> = HashMap::new();
    vulnerabilities.insert("gone".to_string(), vec![(1, "<2.0.0".parse().unwrap())]);
    vulnerabilities.insert("bumped".to_string(), vec![(2, "<2.0.0".parse().unwrap())]);
    vulnerabilities.insert("non-semver".to_string(), vec![(3, "<2.0.0".parse().unwrap())]);

    // `non-semver` is still installed but only under a non-semver key, so its
    // version can't be range-checked.
    let installed = InstalledPackages {
        names: ["bumped".to_string(), "non-semver".to_string()].into_iter().collect(),
        versions: HashMap::from([("bumped".to_string(), vec!["2.0.0".parse().unwrap()])]),
    };

    let (fixed, remaining) =
        report_fixed_remaining(&vulnerabilities, &HashMap::new(), &[], &installed);

    assert!(fixed.contains(&1), "an absent package is fixed");
    assert!(fixed.contains(&2), "a package bumped out of range is fixed");
    assert!(
        remaining.contains(&3),
        "a package surviving only under a non-semver key cannot be proven fixed",
    );
    assert!(!fixed.contains(&3));
}
