use std::{collections::HashMap, fs::read_to_string, io::Write};

use insta::assert_snapshot;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use tempfile::{NamedTempFile, tempdir};

use super::{
    BINDING_GYP, BundleDependencies, InitAuthor, InitOptions, PackageManifest,
    PackageManifestError, apply_runtime_on_fail_override, convert_dependencies_to_engines_runtime,
    convert_engines_runtime_to_dependencies, extract_license, files_build_triggers,
    is_markdown_readme_file_name, is_readme_file_name, manifest_opts_out_of_gyp_build,
    manifest_requires_build, node_version_from_engines_runtime, parse_manifest_bytes,
    pkg_requires_build, safe_read_package_json_from_dir,
};
use crate::DependencyGroup;
use serde_json::json;

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn manifest_from_json(value: serde_json::Value) -> (PackageManifest, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, value.to_string()).unwrap();
    (PackageManifest::from_path(path).unwrap(), dir)
}

#[test]
fn recognizes_npm_readme_file_names() {
    for name in ["README", "README.md", "readme.markdown", "README.mdown", "README.a", "README."] {
        assert!(is_readme_file_name(name), "{name}");
    }
    for name in ["readme", "README.txt", "README.md.bak", "NOTREADME.md", "README.am", "README.aa"]
    {
        assert!(!is_readme_file_name(name), "{name}");
    }
    assert!(!is_markdown_readme_file_name("README"));
    for name in ["README.md", "readme.markdown", "README.mdown", "README.a", "README."] {
        assert!(is_markdown_readme_file_name(name), "{name}");
    }
}

mod behavior;

mod manifests;

mod authorization;

mod dependencies;

mod runtime;

mod files;

mod lockfile;

mod yaml;

mod json5;
