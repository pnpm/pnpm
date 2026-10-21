use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::{PackageBin, parse_shebang_from_bytes, resolve_package_bins, verify_package_bins};

#[test]
fn resolves_string_bin_and_shebang_runtime() {
    let manifest = json!({
        "name": "@scope/cli-tool",
        "version": "1.0.0",
        "bin": "./bin/cli.js",
    });
    let mut archive_files = BTreeMap::new();
    archive_files.insert(
        "bin/cli.js".to_string(),
        parse_shebang_from_bytes(b"#!/usr/bin/env -S node --no-warnings\nconsole.log(1);\n"),
    );

    let bins = resolve_package_bins(&manifest, &archive_files).expect("bins should resolve");
    assert_eq!(bins.len(), 1);
    let entry = bins.get("cli-tool").expect("unscoped bin name");
    assert_eq!(entry.path, "bin/cli.js");
    assert_eq!(entry.prog.as_deref(), Some("node"));
    assert_eq!(entry.args, " --no-warnings");
}

#[test]
fn resolves_directories_bin_and_extension_fallback() {
    let manifest = json!({
        "name": "dir-bins",
        "version": "1.0.0",
        "directories": { "bin": "./bin" },
    });
    let mut archive_files = BTreeMap::new();
    archive_files.insert(
        "bin/alpha.js".to_string(),
        parse_shebang_from_bytes(b"console.log(1);\n"),
    );
    archive_files.insert(
        "bin/sub/beta".to_string(),
        parse_shebang_from_bytes(b"#!/bin/sh\necho hi\n"),
    );
    archive_files.insert("lib/ignored.js".to_string(), None);

    let bins = resolve_package_bins(&manifest, &archive_files).expect("bins should resolve");
    assert_eq!(bins.len(), 2);
    assert_eq!(
        bins.get("alpha.js"),
        Some(&PackageBin {
            path: "bin/alpha.js".to_string(),
            prog: Some("node".to_string()),
            args: String::new(),
        }),
    );
    assert_eq!(
        bins.get("beta"),
        Some(&PackageBin {
            path: "bin/sub/beta".to_string(),
            prog: Some("/bin/sh".to_string()),
            args: String::new(),
        }),
    );
}

#[test]
fn returns_none_when_declared_bin_target_is_missing_from_archive() {
    let manifest = json!({
        "name": "missing-target",
        "version": "1.0.0",
        "bin": "./dist/cli.js",
    });
    let archive_files = BTreeMap::new();
    assert_eq!(resolve_package_bins(&manifest, &archive_files), None);
}

#[test]
fn returns_none_when_bin_path_escapes_package_root() {
    let manifest = json!({
        "name": "escape-target",
        "version": "1.0.0",
        "bin": "../outside.js",
    });
    let archive_files = BTreeMap::new();
    assert_eq!(resolve_package_bins(&manifest, &archive_files), None);
}

#[test]
fn verify_package_bins_checks_manifest_and_file_set() {
    let manifest = json!({
        "name": "cli-tool",
        "version": "1.0.0",
        "bin": { "cli": "./bin/cli.js" },
    });
    let files = HashMap::from([
        ("package.json".to_string(), "cas/pkg"),
        ("bin/cli.js".to_string(), "cas/cli"),
    ]);
    let matching = BTreeMap::from([(
        "cli".to_string(),
        PackageBin {
            path: "bin/cli.js".to_string(),
            prog: Some("node".to_string()),
            args: " --no-warnings".to_string(),
        },
    )]);
    assert!(verify_package_bins(&matching, &manifest, &files));

    let wrong_path = BTreeMap::from([(
        "cli".to_string(),
        PackageBin {
            path: "package.json".to_string(),
            prog: Some("node".to_string()),
            args: String::new(),
        },
    )]);
    assert!(!verify_package_bins(&wrong_path, &manifest, &files));

    let extra_bin = BTreeMap::from([
        (
            "cli".to_string(),
            PackageBin {
                path: "bin/cli.js".to_string(),
                prog: Some("node".to_string()),
                args: String::new(),
            },
        ),
        (
            "injected".to_string(),
            PackageBin {
                path: "bin/cli.js".to_string(),
                prog: Some("node".to_string()),
                args: String::new(),
            },
        ),
    ]);
    assert!(!verify_package_bins(&extra_bin, &manifest, &files));
}
