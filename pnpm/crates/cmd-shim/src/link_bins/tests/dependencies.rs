use super::{
    Arc, Host, LinkBinsOptions, PackageBinSource, create_dir_all, is_shim_pointing_at, json,
    link_bins_of_packages, read_to_string, tempdir, write_file,
};

#[test]
fn same_package_bin_conflict_prefers_latest_version() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    let packages = [("@_ts/min", "5.4.5"), ("typescript", "6.0.2"), ("@_ts/max", "6.0.3")].map(
        |(alias, version)| {
            let location = modules.join(alias);
            create_dir_all(location.join("bin")).unwrap();
            write_file(location.join("bin/tsc"), "#!/usr/bin/env node\n").unwrap();
            PackageBinSource::new(
                location,
                Arc::new(json!({
                    "name": "typescript",
                    "version": version,
                    "bin": { "tsc": "bin/tsc" },
                })),
            )
        },
    );
    let expected_target = modules.join("@_ts/max/bin/tsc");
    let bins = modules.join(".bin");

    link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default()).unwrap();

    let body = read_to_string(bins.join("tsc")).unwrap();
    assert!(
        is_shim_pointing_at(&body, &expected_target),
        "the highest TypeScript version must provide `tsc`, got body:\n{body}",
    );
}
