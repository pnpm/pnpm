use super::{
    BelongsTo, Config, Include, LicenseInfo, LicensesArgs, LicensesDependencyOptions,
    collect_dependencies, compare_package_names, extract_license_author, extract_license_homepage,
    render_package_name, select_newer_version,
};
use pacquet_lockfile::Lockfile;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn test_include_logic() {
    let opts =
        LicensesDependencyOptions { prod: false, dev: false, no_optional: false, optional: false };
    let include = opts.include();
    assert!(include.dependencies);
    assert!(include.dev_dependencies);
    assert!(include.optional_dependencies);

    let opts_prod =
        LicensesDependencyOptions { prod: true, dev: false, no_optional: false, optional: false };
    let include_prod = opts_prod.include();
    assert!(include_prod.dependencies);
    assert!(!include_prod.dev_dependencies);
    assert!(!include_prod.optional_dependencies);

    let opts_no_optional =
        LicensesDependencyOptions { prod: false, dev: false, no_optional: true, optional: false };
    let include_no_optional = opts_no_optional.include();
    assert!(include_no_optional.dependencies);
    assert!(include_no_optional.dev_dependencies);
    assert!(!include_no_optional.optional_dependencies);
}

#[tokio::test]
async fn test_empty_lockfile() {
    let dir = TempDir::new().unwrap();
    let config = Config::default();
    let args = LicensesArgs {
        json: true,
        long: false,
        dependency_options: LicensesDependencyOptions {
            prod: false,
            dev: false,
            no_optional: false,
            optional: false,
        },
        params: vec!["list".to_string()],
    };

    // An empty directory has no lockfile, so it should just print "{}" and exit ok
    let res = args.run(&config, dir.path(), false).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_no_subcommand_matches_pnpm_error_code() {
    let dir = TempDir::new().unwrap();
    let config = Config::default();
    let args = LicensesArgs {
        json: false,
        long: false,
        dependency_options: LicensesDependencyOptions {
            prod: false,
            dev: false,
            no_optional: false,
            optional: false,
        },
        params: vec![],
    };

    let err = args.run(&config, dir.path(), false).await.unwrap_err();
    assert!(format!("{err:?}").contains("ERR_PNPM_LICENCES_NO_SUBCOMMAND"));
}

#[test]
fn collects_every_importer_and_filters_unsupported_subtrees() {
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  .:
    devDependencies:
      dev-only:
        specifier: 1.0.0
        version: 1.0.0
      darwin-only:
        specifier: 1.0.0
        version: 1.0.0
      '@esbuild/darwin-arm64':
        specifier: 1.0.0
        version: 1.0.0
  packages/a:
    dependencies:
      prod-only:
        specifier: 1.0.0
        version: 1.0.0
      required-darwin:
        specifier: 1.0.0
        version: 1.0.0
packages:
  '@esbuild/darwin-arm64@1.0.0':
    resolution: {integrity: sha512-inferred-darwin}
  dev-only@1.0.0:
    resolution: {integrity: sha512-dev}
  darwin-only@1.0.0:
    resolution: {integrity: sha512-darwin}
    os: [darwin]
  hidden-child@1.0.0:
    resolution: {integrity: sha512-hidden}
  prod-only@1.0.0:
    resolution: {integrity: sha512-prod}
  required-child@1.0.0:
    resolution: {integrity: sha512-required-child}
  required-darwin@1.0.0:
    resolution: {integrity: sha512-required-darwin}
    os: [darwin]
  visible-child@1.0.0:
    resolution: {integrity: sha512-visible}
snapshots:
  '@esbuild/darwin-arm64@1.0.0':
    optional: true
  dev-only@1.0.0: {}
  darwin-only@1.0.0:
    optional: true
    dependencies:
      hidden-child: 1.0.0
  hidden-child@1.0.0: {}
  prod-only@1.0.0:
    dependencies:
      visible-child: 1.0.0
  required-child@1.0.0: {}
  required-darwin@1.0.0:
    dependencies:
      required-child: 1.0.0
  visible-child@1.0.0: {}
",
    )
    .unwrap();
    let include =
        Include { dependencies: true, dev_dependencies: true, optional_dependencies: true };

    let dependencies = collect_dependencies(
        &lockfile,
        lockfile.importers.keys(),
        include,
        None,
        "linux",
        "x64",
        "glibc",
    );

    assert_eq!(dependencies.len(), 5);
    assert_eq!(dependencies[&"dev-only@1.0.0".parse().unwrap()], BelongsTo::Dev);
    assert_eq!(dependencies[&"prod-only@1.0.0".parse().unwrap()], BelongsTo::Prod);
    assert_eq!(dependencies[&"required-child@1.0.0".parse().unwrap()], BelongsTo::Prod);
    assert_eq!(dependencies[&"required-darwin@1.0.0".parse().unwrap()], BelongsTo::Prod);
    assert_eq!(dependencies[&"visible-child@1.0.0".parse().unwrap()], BelongsTo::Prod);
}

#[test]
fn renders_dev_classification() {
    let info = LicenseInfo {
        name: "dev-only".to_string(),
        versions: vec!["1.0.0".to_string()],
        paths: Vec::new(),
        license: "MIT".to_string(),
        belongs_to: BelongsTo::Dev,
        selected_version: "1.0.0".to_string(),
        author: None,
        homepage: None,
        description: None,
    };

    let rendered = render_package_name(&info);
    assert!(rendered.starts_with("dev-only "));
    assert!(rendered.contains("(dev)"));
}

#[test]
fn selects_latest_version_classification_with_semver_precedence() {
    let mut info = LicenseInfo {
        name: "multiple-versions".to_string(),
        versions: vec!["2.0.0".to_string()],
        paths: Vec::new(),
        license: "MIT".to_string(),
        belongs_to: BelongsTo::Prod,
        selected_version: "2.0.0".to_string(),
        author: None,
        homepage: None,
        description: None,
    };

    assert!(select_newer_version(&mut info, "10.0.0", BelongsTo::Dev));
    assert_eq!(info.belongs_to, BelongsTo::Dev);
    assert_eq!(info.selected_version, "10.0.0");
    assert!(!select_newer_version(&mut info, "3.0.0", BelongsTo::Prod));
    assert_eq!(info.belongs_to, BelongsTo::Dev);
}

#[test]
fn compares_package_names_like_javascript_locale_compare() {
    let mut names = [
        "string-width",
        "stringify-object",
        "string_decoder",
        "a~b",
        "a/b",
        "a.b",
        "a-b",
        "a_b",
        "a0b",
        "aab",
        "ab",
    ];
    names.sort_by(|left, right| compare_package_names(left, right));
    assert_eq!(
        names,
        [
            "a_b",
            "a-b",
            "a.b",
            "a/b",
            "a~b",
            "a0b",
            "aab",
            "ab",
            "string_decoder",
            "string-width",
            "stringify-object",
        ],
    );
}

#[test]
fn normalizes_author_for_license_reports() {
    assert_eq!(
        extract_license_author(&json!({
            "author": "The Babel Team <team@babel.dev> (https://babel.dev/team)"
        })),
        Some("The Babel Team".to_string()),
    );
    assert_eq!(
        extract_license_author(&json!({ "author": { "name": "The Babel Team" } })),
        Some("The Babel Team".to_string()),
    );
    assert_eq!(extract_license_author(&json!({ "author": "" })), Some(String::new()));
}

#[test]
fn normalizes_homepage_for_license_reports() {
    assert_eq!(
        extract_license_homepage(&json!({ "homepage": "babel.dev" })),
        Some("http://babel.dev".to_string()),
    );
    assert_eq!(
        extract_license_homepage(&json!({
            "repository": "git+https://github.com/babel/babel.git"
        })),
        Some("https://github.com/babel/babel#readme".to_string()),
    );
}
