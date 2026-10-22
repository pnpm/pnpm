use super::{TempDir, WORKSPACE_MANIFEST_FILENAME, fs};
use pnpm_config::PermissionCapability::{self, Build, Skills};

fn run(original: Option<&str>, entries: &[(&str, PermissionCapability, bool)]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_permissions(dir.path(), entries.iter().copied()).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn build_decisions_go_to_allow_builds_without_a_permissions_block() {
    let out =
        run(Some("packages:\n  - pkgs/*\n"), &[("esbuild", Build, true), ("sharp", Build, false)]);
    assert_eq!(
        out.as_deref(),
        Some("packages:\n  - pkgs/*\nallowBuilds:\n  esbuild: true\n  sharp: false\n"),
    );
}

#[test]
fn skills_decisions_create_the_permissions_block() {
    let out = run(None, &[("drizzle-kit", Build, true), ("drizzle-kit", Skills, true)]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds:\n  drizzle-kit: true\npermissions:\n  drizzle-kit:\n    skills: true\n"),
    );
}

#[test]
fn build_decisions_join_an_existing_permissions_block() {
    let out = run(
        Some(
            "allowBuilds:\n  esbuild: false\n  sharp: true\n\npermissions:\n  drizzle-kit:\n    skills: true\n",
        ),
        &[("esbuild", Build, true), ("drizzle-kit", Build, false), ("@acme/kit", Skills, false)],
    );
    assert_eq!(
        out.as_deref(),
        Some(concat!(
            "allowBuilds:\n  sharp: true\n\n",
            "permissions:\n",
            "  '@acme/kit':\n    skills: false\n",
            "  drizzle-kit:\n    build: false\n    skills: true\n",
            "  esbuild:\n    build: true\n",
        )),
    );
}

#[test]
fn removes_allow_builds_when_its_last_entry_moves() {
    let out =
        run(Some("allowBuilds:\n  esbuild: false\npermissions: {}\n"), &[("esbuild", Build, true)]);
    assert_eq!(out.as_deref(), Some("permissions: { esbuild: { build: true } }\n"));
}

#[test]
fn rewrites_a_changed_decision_in_place() {
    let original = "permissions:\n  foo:\n    skills: false # reviewed\n";
    assert_eq!(
        run(Some(original), &[("foo", Skills, true)]).as_deref(),
        Some("permissions:\n  foo:\n    skills: true # reviewed\n"),
    );
    assert_eq!(run(Some(original), &[("foo", Skills, false)]).as_deref(), Some(original));
}
