use std::{collections::HashMap, fs::read_to_string, io::Write};

use insta::assert_snapshot;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use tempfile::{NamedTempFile, tempdir};

use super::{
    BINDING_GYP, BundleDependencies, InitAuthor, InitOptions, PackageManifest,
    PackageManifestError, ReadmeKind, apply_runtime_on_fail_override,
    convert_dependencies_to_engines_runtime, convert_engines_runtime_to_dependencies,
    extract_license, files_build_triggers, is_preferred_readme, manifest_opts_out_of_gyp_build,
    manifest_requires_build, node_version_from_engines_runtime, parse_manifest_bytes,
    pkg_requires_build, readme_kind, safe_read_package_json_from_dir,
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
    for name in ["README.md", "readme.MD"] {
        assert_eq!(readme_kind(name), Some(ReadmeKind::ReadmeMd), "{name}");
    }
    for name in ["readme.markdown", "README.mdown", "README.a", "README.", "README.x.md"] {
        assert_eq!(readme_kind(name), Some(ReadmeKind::Markdown), "{name}");
    }
    assert_eq!(readme_kind("README"), Some(ReadmeKind::Bare));
    for name in ["readme", "README.txt", "README.md.bak", "NOTREADME.md", "README.am", "README.aa"]
    {
        assert_eq!(readme_kind(name), None, "{name}");
    }
}

#[test]
fn prefers_higher_readme_kinds_then_lower_file_names() {
    assert!(is_preferred_readme((ReadmeKind::Bare, "README"), None));
    assert!(is_preferred_readme(
        (ReadmeKind::ReadmeMd, "README.md"),
        Some((ReadmeKind::Markdown, "README.a"))
    ));
    assert!(!is_preferred_readme(
        (ReadmeKind::Bare, "README"),
        Some((ReadmeKind::Markdown, "README.mdown"))
    ));
    assert!(is_preferred_readme(
        (ReadmeKind::Markdown, "README.markdown"),
        Some((ReadmeKind::Markdown, "README.mdown"))
    ));
    assert!(!is_preferred_readme(
        (ReadmeKind::Markdown, "README.mdown"),
        Some((ReadmeKind::Markdown, "README.markdown"))
    ));
    assert!(is_preferred_readme(
        (ReadmeKind::ReadmeMd, "README.md"),
        Some((ReadmeKind::ReadmeMd, "README.md"))
    ));
}

/// UTF-8 byte order puts U+E000 before U+1F600; UTF-16 code-unit order,
/// which the TypeScript CLI uses, puts it after.
#[test]
fn breaks_readme_ties_in_utf16_order() {
    assert!(is_preferred_readme(
        (ReadmeKind::Markdown, "README.\u{1F600}.md"),
        Some((ReadmeKind::Markdown, "README.\u{E000}.md"))
    ));
    assert!(!is_preferred_readme(
        (ReadmeKind::Markdown, "README.\u{E000}.md"),
        Some((ReadmeKind::Markdown, "README.\u{1F600}.md"))
    ));
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
