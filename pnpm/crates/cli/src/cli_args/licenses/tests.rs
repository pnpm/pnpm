use super::*;
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
    };

    // An empty directory has no lockfile, so it should just print "{}" and exit ok
    let res = args.run(&config, dir.path(), false).await;
    assert!(res.is_ok());
}
