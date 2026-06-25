use super::*;
use pacquet_lockfile::{EnvLockfile, Lockfile, SnapshotEntry, SpecifierAndResolution};

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
