use super::{
    Host, LinkBinsOptions, create_dir_all, json, link_bins, read_to_string, tempdir, write_file,
};
use std::{
    fs,
    time::{Duration, SystemTime},
};

#[test]
fn forced_refresh_preserves_unchanged_shims() {
    let directory = tempdir().unwrap();
    let modules = directory.path().join("node_modules");
    for name in ["changed", "stable"] {
        let package = modules.join(name);
        create_dir_all(&package).unwrap();
        write_file(
            package.join("package.json"),
            json!({"name": name, "bin": "cli.js"}).to_string(),
        )
        .unwrap();
        write_file(package.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    }
    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();
    let unchanged = bins.join("stable");
    let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    fs::File::options()
        .write(true)
        .open(&unchanged)
        .unwrap()
        .set_modified(timestamp)
        .unwrap();
    let original = read_to_string(bins.join("changed")).unwrap();
    write_file(modules.join("changed/cli.js"), "#!/usr/bin/env node --no-warnings\n").unwrap();
    link_bins::<Host>(
        &modules,
        &bins,
        &LinkBinsOptions { force: true, ..LinkBinsOptions::default() },
    )
    .unwrap();
    assert_eq!(
        fs::metadata(&unchanged)
            .unwrap()
            .modified()
            .unwrap(),
        timestamp,
    );
    let refreshed = read_to_string(bins.join("changed")).unwrap();
    assert_ne!(refreshed, original);
    assert!(refreshed.contains("--no-warnings"), "refreshed shim: {refreshed}");
}
