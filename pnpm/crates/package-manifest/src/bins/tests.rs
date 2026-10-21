use std::collections::{BTreeMap, HashMap};

use serde_json::json;
use tempfile::tempdir;

use super::{
    MAX_PRECALCULATED_BINS, PackageBin, normalize_package_relative_path, parse_shebang_from_bytes,
    resolve_package_bins, serialize_precalculated_bins, verify_package_bins,
};

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
    for raw in ["../outside.js", r"..\outside.js", r"C:\outside.js", r"bin\cli.js", "/abs/cli.js"] {
        assert_eq!(normalize_package_relative_path(raw), None, "expected {raw:?} to be rejected");
        let manifest = json!({
            "name": "escape-target",
            "version": "1.0.0",
            "bin": raw,
        });
        let archive_files = BTreeMap::new();
        assert_eq!(resolve_package_bins(&manifest, &archive_files), None);
    }
}

#[test]
fn shebang_env_prefix_requires_word_boundary() {
    let runtime =
        parse_shebang_from_bytes(b"#!/usr/bin/envsubst --flag\n").expect("shebang should parse");
    assert_eq!(runtime.prog.as_deref(), Some("/usr/bin/envsubst"));
    assert_eq!(runtime.args, " --flag");
}

#[test]
fn verify_package_bins_checks_manifest_and_file_set() {
    let dir = tempdir().expect("tempdir");
    let pkg_path = dir.path().join("pkg.json");
    let cli_path = dir.path().join("cli.js");
    std::fs::write(&pkg_path, br#"{"name":"cli-tool"}"#).expect("write pkg");
    std::fs::write(&cli_path, b"#!/usr/bin/env -S node --no-warnings\nconsole.log(1);\n")
        .expect("write cli");

    let manifest = json!({
        "name": "cli-tool",
        "version": "1.0.0",
        "bin": { "cli": "./bin/cli.js" },
    });
    let files = HashMap::from([
        ("package.json".to_string(), pkg_path),
        ("bin/cli.js".to_string(), cli_path),
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

    let wrong_prog = BTreeMap::from([(
        "cli".to_string(),
        PackageBin {
            path: "bin/cli.js".to_string(),
            prog: Some("sh".to_string()),
            args: " --no-warnings".to_string(),
        },
    )]);
    assert!(!verify_package_bins(&wrong_prog, &manifest, &files));

    let wrong_args = BTreeMap::from([(
        "cli".to_string(),
        PackageBin {
            path: "bin/cli.js".to_string(),
            prog: Some("node".to_string()),
            args: " --inspect".to_string(),
        },
    )]);
    assert!(!verify_package_bins(&wrong_args, &manifest, &files));

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
                args: " --no-warnings".to_string(),
            },
        ),
        (
            "injected".to_string(),
            PackageBin {
                path: "bin/cli.js".to_string(),
                prog: Some("node".to_string()),
                args: " --no-warnings".to_string(),
            },
        ),
    ]);
    assert!(!verify_package_bins(&extra_bin, &manifest, &files));
}

#[test]
fn caps_precalculated_bins_count_and_serialized_size() {
    let manifest = json!({
        "name": "many-bins",
        "version": "1.0.0",
        "directories": { "bin": "./bin" },
    });
    let mut at_limit = BTreeMap::new();
    for index in 0..MAX_PRECALCULATED_BINS {
        at_limit.insert(format!("bin/cmd-{index}.js"), None);
    }
    let resolved = resolve_package_bins(&manifest, &at_limit).expect("64 bins should fit budget");
    assert_eq!(resolved.len(), MAX_PRECALCULATED_BINS);
    assert!(serialize_precalculated_bins(&resolved).is_some());

    let mut over_count = at_limit;
    over_count.insert(format!("bin/cmd-{MAX_PRECALCULATED_BINS}.js"), None);
    assert_eq!(resolve_package_bins(&manifest, &over_count), None);

    let long_subpath = "sub/".repeat(60);
    let mut over_bytes = BTreeMap::new();
    for index in 0..40 {
        over_bytes.insert(format!("bin/{long_subpath}command-name-{index}.js"), None);
    }
    assert_eq!(resolve_package_bins(&manifest, &over_bytes), None);
}
