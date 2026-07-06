use super::{link_cached_engine_bins, package_dir};
use std::fs;

#[test]
fn cache_hit_relinks_missing_pnpm_bin() {
    let root = tempfile::TempDir::new().expect("tmp dir");
    let slot = root.path().join("slot");
    let pkg_dir = package_dir(&slot, "pnpm");
    fs::create_dir_all(pkg_dir.join("bin")).expect("create package bin dir");
    fs::write(
        pkg_dir.join("package.json"),
        r#"{"name":"pnpm","version":"6.16.0","bin":{"pnpm":"bin/pnpm.cjs"}}"#,
    )
    .expect("write manifest");
    fs::write(pkg_dir.join("bin").join("pnpm.cjs"), "#!/usr/bin/env node\n")
        .expect("write pnpm bin");
    let bin_dir = slot.join("bin");
    fs::create_dir_all(&bin_dir).expect("create stale bin dir");

    let linked = link_cached_engine_bins(&slot, "pnpm", false).expect("link bins");

    assert_eq!(linked, bin_dir);
    assert!(bin_dir.join("pnpm").exists());
}
