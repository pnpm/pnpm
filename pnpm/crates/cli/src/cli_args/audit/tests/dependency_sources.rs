use super::{
    BTreeMap, Config, HashSet, LockedPackageRegistry, RawBulkAdvisory, all_dependencies,
    bulk_response_to_audit_report, locked_package_registries, lockfile_to_audit_request,
    package_version, parse_lockfile, prod_without_optional,
};

#[test]
fn package_version_reads_tarball_file_git_and_named_registry_versions() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:
  .: {}

packages:
  http-pkg@http://127.0.0.1:8080/http-pkg-1.0.0.tgz:
    resolution: {tarball: http://127.0.0.1:8080/http-pkg-1.0.0.tgz}
    version: 1.0.0
  file-tgz@file:./file-tgz-1.2.3.tgz:
    resolution: {tarball: file:./file-tgz-1.2.3.tgz}
    version: 1.2.3
  git-pkg@git+https://example.com/repo.git#0123456789abcdef:
    resolution: {repo: https://example.com/repo.git, commit: 0123456789abcdef, type: git}
    version: 2.3.4
  named-pkg@work:3.4.5:
    resolution: {integrity: sha512-YWJj}
  '@scope/pkg@work:3.4.5':
    resolution: {integrity: sha512-YWJj}
  tampered-reg@1.0.0:
    resolution: {integrity: sha512-YWJj}
    version: 99.0.0
  tampered-named@work:1.0.0:
    resolution: {integrity: sha512-YWJj}
    version: 99.0.0
  ws-pkg@file:packages/ws-pkg:
    resolution: {directory: packages/ws-pkg, type: directory}
    version: 9.9.9
  dir-semver@1.0.0:
    resolution: {directory: packages/dir-semver, type: directory}
  unversioned@http://127.0.0.1:8080/unversioned.tgz:
    resolution: {tarball: http://127.0.0.1:8080/unversioned.tgz}
  bad-version@http://127.0.0.1:8080/bad-version.tgz:
    resolution: {tarball: http://127.0.0.1:8080/bad-version.tgz}
    version: not-a-semver
",
    );
    let packages = lockfile.packages.as_ref();
    let version_of = |raw: &str| package_version(&raw.parse().expect("valid PackageKey"), packages);

    assert_eq!(
        version_of("http-pkg@http://127.0.0.1:8080/http-pkg-1.0.0.tgz"),
        Some("1.0.0".to_string()),
    );
    assert_eq!(
        version_of("http-pkg@http://127.0.0.1:8080/http-pkg-1.0.0.tgz(peer@2.0.0)"),
        Some("1.0.0".to_string()),
    );
    assert_eq!(version_of("file-tgz@file:./file-tgz-1.2.3.tgz"), Some("1.2.3".to_string()));
    assert_eq!(
        version_of("git-pkg@git+https://example.com/repo.git#0123456789abcdef"),
        Some("2.3.4".to_string()),
    );
    assert_eq!(version_of("named-pkg@work:3.4.5"), Some("3.4.5".to_string()));
    assert_eq!(version_of("@scope/pkg@work:3.4.5(peer@1.0.0)"), Some("3.4.5".to_string()));
    assert_eq!(version_of("tampered-reg@1.0.0"), Some("1.0.0".to_string()));
    assert_eq!(version_of("tampered-named@work:1.0.0"), Some("1.0.0".to_string()));
    assert_eq!(version_of("ws-pkg@file:packages/ws-pkg"), None);
    assert_eq!(version_of("dir-semver@1.0.0"), None);
    assert_eq!(version_of("node@runtime:24.0.0"), None);
    assert_eq!(version_of("unversioned@http://127.0.0.1:8080/unversioned.tgz"), None);
    assert_eq!(version_of("bad-version@http://127.0.0.1:8080/bad-version.tgz"), None);
}

#[test]
fn audit_request_and_report_include_direct_and_transitive_non_semver_and_named_registry_deps() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      http-parent:
        specifier: http://127.0.0.1:8080/http-parent-1.0.0.tgz
        version: http://127.0.0.1:8080/http-parent-1.0.0.tgz
      file-tgz:
        specifier: file:./tarballs/file-tgz-1.2.0.tgz
        version: file:./tarballs/file-tgz-1.2.0.tgz
      git-parent:
        specifier: git+https://example.com/repo.git#deadbeef
        version: git+https://example.com/repo.git#deadbeef
      ws-lib:
        specifier: workspace:*
        version: file:packages/ws-lib
    devDependencies:
      work-alias:
        specifier: work:aliased-named@^1.4.0
        version: aliased-named@work:1.4.0
    optionalDependencies:
      opt-tgz:
        specifier: file:./tarballs/opt-tgz-1.5.0.tgz
        version: file:./tarballs/opt-tgz-1.5.0.tgz

packages:
  http-parent@http://127.0.0.1:8080/http-parent-1.0.0.tgz:
    resolution: {tarball: http://127.0.0.1:8080/http-parent-1.0.0.tgz}
    version: 1.0.0
  http-child@http://127.0.0.1:8080/http-child-1.1.0.tgz:
    resolution: {tarball: http://127.0.0.1:8080/http-child-1.1.0.tgz}
    version: 1.1.0
  file-tgz@file:./tarballs/file-tgz-1.2.0.tgz:
    resolution: {tarball: file:./tarballs/file-tgz-1.2.0.tgz}
    version: 1.2.0
  git-parent@git+https://example.com/repo.git#deadbeef:
    resolution: {repo: https://example.com/repo.git, commit: deadbeef, type: git}
    version: 1.3.0
  '@acme/named@work:1.3.0':
    resolution: {integrity: sha512-YWJj}
  aliased-named@work:1.4.0:
    resolution: {integrity: sha512-YWJj}
  opt-tgz@file:./tarballs/opt-tgz-1.5.0.tgz:
    resolution: {tarball: file:./tarballs/opt-tgz-1.5.0.tgz}
    version: 1.5.0
  ws-lib@file:packages/ws-lib:
    resolution: {directory: packages/ws-lib, type: directory}
  via-ws@1.6.0:
    resolution: {integrity: sha512-YWJj}

snapshots:
  http-parent@http://127.0.0.1:8080/http-parent-1.0.0.tgz:
    dependencies:
      http-child: http://127.0.0.1:8080/http-child-1.1.0.tgz
  http-child@http://127.0.0.1:8080/http-child-1.1.0.tgz: {}
  file-tgz@file:./tarballs/file-tgz-1.2.0.tgz: {}
  git-parent@git+https://example.com/repo.git#deadbeef:
    dependencies:
      '@acme/named': work:1.3.0
  '@acme/named@work:1.3.0': {}
  aliased-named@work:1.4.0: {}
  opt-tgz@file:./tarballs/opt-tgz-1.5.0.tgz:
    optional: true
  ws-lib@file:packages/ws-lib:
    dependencies:
      via-ws: 1.6.0
  via-ws@1.6.0: {}
",
    );

    let full_request = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    assert_eq!(full_request.request["http-parent"], vec!["1.0.0"]);
    assert_eq!(full_request.request["http-child"], vec!["1.1.0"]);
    assert_eq!(full_request.request["file-tgz"], vec!["1.2.0"]);
    assert_eq!(full_request.request["git-parent"], vec!["1.3.0"]);
    assert_eq!(full_request.request["@acme/named"], vec!["1.3.0"]);
    assert_eq!(full_request.request["aliased-named"], vec!["1.4.0"]);
    assert_eq!(full_request.request["opt-tgz"], vec!["1.5.0"]);
    assert_eq!(full_request.request["via-ws"], vec!["1.6.0"]);
    assert!(!full_request.request.contains_key("ws-lib"));
    assert_eq!(full_request.total_dependencies, 8);
    assert_eq!(full_request.dependencies, 6);
    assert_eq!(full_request.dev_dependencies, 1);
    assert_eq!(full_request.optional_dependencies, 1);

    let prod_request = lockfile_to_audit_request(&lockfile, None, prod_without_optional());
    assert!(!prod_request.request.contains_key("aliased-named"));
    assert!(!prod_request.request.contains_key("opt-tgz"));
    assert_eq!(prod_request.total_dependencies, 6);

    let bulk = [
        ("http-parent", 1001_u64, "<=1.0.0"),
        ("http-child", 1002, "<=1.1.0"),
        ("file-tgz", 1003, "<=1.2.0"),
        ("git-parent", 1004, "<=1.3.0"),
        ("@acme/named", 1005, "<=1.3.0"),
        ("aliased-named", 1006, "<=1.4.0"),
        ("via-ws", 1007, "<=1.6.0"),
    ]
    .into_iter()
    .map(|(module, id, vulnerable_versions)| {
        (
            module.to_string(),
            vec![RawBulkAdvisory {
                id: Some(serde_json::json!(id)),
                url: Some(format!("https://github.com/advisories/GHSA-0000-0000-{id}")),
                title: Some(format!("{module} advisory")),
                severity: Some("critical".to_string()),
                vulnerable_versions: vulnerable_versions.to_string(),
                cwe: None,
            }],
        )
    })
    .collect::<BTreeMap<_, _>>();

    let report =
        bulk_response_to_audit_report(bulk, &full_request, &lockfile, None, all_dependencies());
    assert_eq!(report.metadata.vulnerabilities.critical, 7);
    assert_eq!(report.advisories["1001"].findings[0].paths, vec![".>http-parent"]);
    assert_eq!(report.advisories["1002"].findings[0].paths, vec![".>http-parent>http-child"]);
    assert_eq!(report.advisories["1003"].findings[0].paths, vec![".>file-tgz"]);
    assert_eq!(report.advisories["1004"].findings[0].paths, vec![".>git-parent"]);
    assert_eq!(report.advisories["1005"].findings[0].paths, vec![".>git-parent>@acme/named"]);
    assert_eq!(report.advisories["1006"].findings[0].paths, vec![".>aliased-named"]);
    assert!(report.advisories["1006"].findings[0].dev);
    assert_eq!(report.advisories["1007"].findings[0].paths, vec![".>ws-lib>via-ws"]);
}

#[test]
fn locked_package_registries_distinguishes_single_and_mixed_registry_origins() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      only-work:
        specifier: work:1.0.0
        version: work:1.0.0
      mixed-default:
        specifier: npm:mixed-pkg@1.0.0
        version: mixed-pkg@1.0.0
      mixed-work:
        specifier: work:mixed-pkg@2.0.0
        version: mixed-pkg@work:2.0.0
      dir-semver:
        specifier: workspace:*
        version: 1.0.0
      tarball-only:
        specifier: https://example.com/tarball-only-1.0.0.tgz
        version: https://example.com/tarball-only-1.0.0.tgz

packages:
  only-work@work:1.0.0:
    resolution: {integrity: sha512-YWJj}
  mixed-pkg@1.0.0:
    resolution: {integrity: sha512-YWJj}
  mixed-pkg@work:2.0.0:
    resolution: {integrity: sha512-ZGVm}
  dir-semver@1.0.0:
    resolution: {directory: packages/dir-semver, type: directory}
  tarball-only@https://example.com/tarball-only-1.0.0.tgz:
    resolution: {tarball: https://example.com/tarball-only-1.0.0.tgz}
    version: 1.0.0

snapshots:
  only-work@work:1.0.0: {}
  mixed-pkg@1.0.0: {}
  mixed-pkg@work:2.0.0: {}
  dir-semver@1.0.0: {}
  tarball-only@https://example.com/tarball-only-1.0.0.tgz: {}
",
    );
    let mut config = Config::new();
    config.registry = "https://default.example.com/".to_string();
    config.registries_by_prefix.insert("work".to_string(), "https://work.example.com/".to_string());

    let names = HashSet::from(["only-work", "mixed-pkg", "dir-semver", "tarball-only"]);
    let resolved = locked_package_registries(&lockfile, None, all_dependencies(), &config, &names);

    assert_eq!(
        resolved.get("only-work"),
        Some(&LockedPackageRegistry::Single("https://work.example.com/".to_string())),
    );
    assert_eq!(resolved.get("mixed-pkg"), Some(&LockedPackageRegistry::Ambiguous));
    assert!(!resolved.contains_key("dir-semver"));
    assert!(!resolved.contains_key("tarball-only"));
}
