use super::{CommandTempCwd, Path, advisory_response, fs, pacquet_cmd};
use assert_cmd::assert::OutputAssertExt;

const VULNERABLE_PKG: &str = "@pnpm.e2e/multi-version-a";
const UNRELATED_PKG: &str = "@pnpm.e2e/multi-version-b";

/// The lockfile pins a vulnerable `multi-version-a@2.0.0`, a safe copy of the
/// same package at 1.0.0, and `multi-version-b@3.0.0`, and every range admits
/// a newer version. Only the vulnerable version may move.
#[test]
fn audit_fix_update_re_resolves_only_vulnerable_versions() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();

    write_manifest(&workspace, ["2.0.0", "1.0.0", "3.0.0"]);
    pacquet_cmd(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, ["^2.0.0", "^1.0.0", "^3.0.0"]);
    pacquet_cmd(&workspace, ["install"]).assert().success();
    let lockfile = read_lockfile(&workspace);
    eprintln!("LOCKFILE BEFORE AUDIT:\n{lockfile}");
    for locked in ["multi-version-a@2.0.0", "multi-version-a@1.0.0", "multi-version-b@3.0.0"] {
        assert!(lockfile.contains(locked), "the install should keep {locked} locked");
    }

    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            VULNERABLE_PKG,
            9001,
            "high",
            ">=2.0.0 <2.1.0",
            "vulnerable 2.0.x",
            "GHSA-mult-1111-2222",
        ))
        .create();
    fs::write(
        workspace.join(".npmrc"),
        format!(
            "registry={audit}\n@pnpm.e2e:registry={pnpr}\nstore-dir=../pacquet-store\ncache-dir=../pacquet-cache\nfetchRetries=0\n",
            audit = audit_registry.url(),
            pnpr = pnpr_url,
        ),
    )
    .expect("rewrite .npmrc");

    let output = pacquet_cmd(&workspace, ["audit", "--fix", "update"])
        .output()
        .expect("run audit --fix update");
    assert!(
        output.status.success(),
        "audit --fix update should succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let lockfile = read_lockfile(&workspace);
    eprintln!("LOCKFILE AFTER AUDIT:\n{lockfile}");
    for kept in ["multi-version-a@1.0.0", "multi-version-a@2.1.0", "multi-version-b@3.0.0"] {
        assert!(lockfile.contains(kept), "the lockfile should hold {kept}");
    }
    for moved in ["multi-version-a@1.0.1", "multi-version-a@2.0.0", "multi-version-b@3.1.0"] {
        assert!(!lockfile.contains(moved), "the lockfile should not hold {moved}");
    }
    assert_eq!(
        read_dependencies(&workspace),
        serde_json::json!({
            VULNERABLE_PKG: "^2.1.0",
            "multi-version-a-1": format!("npm:{VULNERABLE_PKG}@^1.0.0"),
            UNRELATED_PKG: "^3.0.0",
        }),
    );
    mock.assert();
    drop((root, npmrc_info));
}

/// Declare the vulnerable package, a safe copy of it under an alias, and an
/// unrelated package, with the given specifiers in that order.
fn write_manifest(workspace: &Path, [vulnerable, safe_copy, unrelated]: [&str; 3]) {
    let manifest = serde_json::json!({
        "name": "audit-fix-update",
        "version": "1.0.0",
        "dependencies": {
            VULNERABLE_PKG: vulnerable,
            "multi-version-a-1": format!("npm:{VULNERABLE_PKG}@{safe_copy}"),
            UNRELATED_PKG: unrelated,
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn read_lockfile(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile")
}

fn read_dependencies(workspace: &Path) -> serde_json::Value {
    let manifest = fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    serde_json::from_str::<serde_json::Value>(&manifest).expect("parse package.json")
        ["dependencies"]
        .take()
}
