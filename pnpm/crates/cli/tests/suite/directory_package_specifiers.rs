//! The `specifiers` a directory package's lockfile entry records (see
//! `pnpm_lockfile::DeclaredSpecifiers`), and how a lockfile written without
//! them is checked.

use crate::_utils::{
    ManifestDeps, WorkspaceFixture, assert_success, read_lockfile, set_dependency,
};
use std::{collections::HashMap, fs};

const LIB_KEY: &str = "lib@file:packages/lib";

/// `app` injects `lib`, which has a dependency, an optional dependency, and
/// a peer that `app` provides. Deduplication is off so the injected copy is
/// a `file:` package rather than a link to the project.
fn injected_workspace() -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("injectWorkspacePackages: true\ndedupeInjectedDeps: false\n");
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.project(
        "lib",
        "lib",
        ManifestDeps {
            prod: &[("@pnpm.e2e/foo", "100.0.0")],
            optional: &[("@pnpm.e2e/bar", "100.0.0")],
            peer: &[("@pnpm.e2e/dep-of-pkg-with-1-dep", "^100.0.0")],
            ..ManifestDeps::default()
        },
    );
    fixture.project(
        "app",
        "app",
        ManifestDeps {
            prod: &[("lib", "workspace:*"), ("@pnpm.e2e/dep-of-pkg-with-1-dep", "100.0.0")],
            ..ManifestDeps::default()
        },
    );
    fixture
}

fn install(fixture: &WorkspaceFixture, args: &[&str]) {
    let output = fixture.command_at(&fixture.workspace, std::iter::once(&"install").chain(args));
    assert_success(&output);
}

fn recorded_specifiers(fixture: &WorkspaceFixture) -> Option<pnpm_lockfile::DeclaredSpecifiers> {
    let lockfile = read_lockfile(&fixture.workspace.join("pnpm-lock.yaml"));
    let key = LIB_KEY.parse().expect("parse the package key");
    lockfile.packages.as_ref().expect("the lockfile has packages")[&key]
        .specifiers
        .clone()
}

fn specifiers(entries: &[(&str, &str)]) -> Option<HashMap<String, String>> {
    Some(
        entries
            .iter()
            .map(|(alias, specifier)| (alias.to_string(), specifier.to_string()))
            .collect(),
    )
}

#[test]
fn an_install_records_the_specifiers_a_directory_package_declares() {
    let fixture = injected_workspace();
    install(&fixture, &[]);

    let recorded = recorded_specifiers(&fixture).expect("the directory package records them");
    assert_eq!(recorded.dependencies, specifiers(&[("@pnpm.e2e/foo", "100.0.0")]));
    assert_eq!(recorded.optional_dependencies, specifiers(&[("@pnpm.e2e/bar", "100.0.0")]));
    let text = fs::read_to_string(fixture.workspace.join("pnpm-lock.yaml")).expect("read");
    assert!(
        text.contains("\n    specifiers:\n      dependencies:\n"),
        "the record follows the declared peers:\n{text}",
    );

    install(&fixture, &["--frozen-lockfile"]);
}

/// A lockfile may record no `specifiers` for a directory package. The check
/// then derives the package's children from its manifest, and the next
/// install that resolves dependencies records them.
#[test]
fn a_lockfile_without_recorded_specifiers_is_checked_from_the_manifest() {
    let fixture = injected_workspace();
    install(&fixture, &[]);
    let lockfile_path = fixture.workspace.join("pnpm-lock.yaml");
    let written = fs::read_to_string(&lockfile_path).expect("read the lockfile");
    let stripped = without_specifiers(&written);
    assert_ne!(stripped, written, "the install recorded specifiers");
    fs::write(&lockfile_path, &stripped).expect("write the lockfile");

    install(&fixture, &["--frozen-lockfile"]);
    assert_eq!(
        fs::read_to_string(&lockfile_path).expect("read the lockfile"),
        stripped,
        "a frozen install keeps the lockfile as it is",
    );
    assert!(recorded_specifiers(&fixture).is_none());

    let lib = fixture.workspace.join("packages/lib");
    set_dependency(&lib, "dependencies", "@pnpm.e2e/foo", "100.1.0");
    let output = fixture.command_at(&fixture.workspace, ["install", "--frozen-lockfile"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
        "the derived check sees the changed dependency:\n{stderr}",
    );

    install(&fixture, &[]);
    let recorded = recorded_specifiers(&fixture).expect("the resolving install records them");
    assert_eq!(recorded.dependencies, specifiers(&[("@pnpm.e2e/foo", "100.1.0")]));
    install(&fixture, &["--frozen-lockfile"]);
}

/// `text` without the `specifiers:` block of every package entry.
fn without_specifiers(text: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in text.lines() {
        if line == "    specifiers:" {
            in_block = true;
            continue;
        }
        if in_block && line.starts_with("      ") {
            continue;
        }
        in_block = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// No project manifest describes an injected directory outside the workspace
/// patterns, so the repeat-install fast path cannot vouch for it: an install
/// after an edit to it resolves again
/// ([pnpm/pnpm#17476](https://github.com/pnpm/pnpm/issues/17476)).
#[test]
fn an_install_sees_a_changed_injected_directory_outside_the_patterns() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("injectWorkspacePackages: true\n");
    let outside = fixture.workspace
        .parent()
        .expect("the workspace has a parent")
        .join("lib");
    fs::create_dir_all(&outside).expect("create the directory outside the workspace");
    fs::write(
        outside.join("package.json"),
        r#"{ "name": "lib", "version": "1.0.0", "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }"#,
    )
    .expect("write the manifest");
    fixture.write_root_manifest(
        "app",
        ManifestDeps { prod: &[("lib", "workspace:../lib")], ..ManifestDeps::default() },
    );
    install(&fixture, &[]);

    set_dependency(&outside, "dependencies", "@pnpm.e2e/foo", "100.1.0");
    install(&fixture, &[]);

    let lockfile = read_lockfile(&fixture.workspace.join("pnpm-lock.yaml"));
    let key: pnpm_lockfile::PackageKey = "lib@file:../lib".parse().expect("parse the package key");
    let recorded = lockfile.packages.as_ref().expect("the lockfile has packages")[&key]
        .specifiers
        .clone()
        .expect("the directory package records its specifiers");
    assert_eq!(recorded.dependencies, specifiers(&[("@pnpm.e2e/foo", "100.1.0")]));
    install(&fixture, &["--frozen-lockfile"]);
}

/// A resolving install that leaves a directory package as it is records the
/// ranges its manifest declares, not the versions the lockfile had locked
/// for them.
#[test]
fn a_resolving_install_keeps_the_recorded_ranges_of_an_unchanged_directory_package() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("injectWorkspacePackages: true\ndedupeInjectedDeps: false\n");
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.project(
        "lib",
        "lib",
        ManifestDeps {
            prod: &[("@pnpm.e2e/foo", "^100.0.0")],
            peer: &[("@pnpm.e2e/dep-of-pkg-with-1-dep", "^100.0.0")],
            ..ManifestDeps::default()
        },
    );
    let app = fixture.project(
        "app",
        "app",
        ManifestDeps {
            prod: &[("lib", "workspace:*"), ("@pnpm.e2e/dep-of-pkg-with-1-dep", "100.0.0")],
            ..ManifestDeps::default()
        },
    );
    install(&fixture, &[]);
    let recorded = recorded_specifiers(&fixture).expect("the directory package records them");
    assert_eq!(recorded.dependencies, specifiers(&[("@pnpm.e2e/foo", "^100.0.0")]));

    set_dependency(&app, "dependencies", "@pnpm.e2e/bar", "100.0.0");
    install(&fixture, &[]);
    let recorded = recorded_specifiers(&fixture).expect("the record survives the resolution");
    assert_eq!(recorded.dependencies, specifiers(&[("@pnpm.e2e/foo", "^100.0.0")]));
    install(&fixture, &["--frozen-lockfile"]);
}
