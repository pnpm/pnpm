use super::{
    HashSet, copy_fixture, dedicated_workspace_with_reachable_project, fs, pacquet, run_sbom_json,
    split_root_names,
};

#[test]
fn split_and_filtered_sbom_read_per_project_workspace_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile = fs::read(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        fs::write(project_dir.join("pnpm-lock.yaml"), &lockfile).expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let split =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run split pacquet sbom");
    assert!(
        split.status.success(),
        "split SBOM failed: {}",
        String::from_utf8_lossy(&split.stderr),
    );
    let mut names = String::from_utf8(split.stdout)
        .expect("split stdout is UTF-8")
        .lines()
        .map(|line| {
            let sbom: serde_json::Value = serde_json::from_str(line).expect("parse NDJSON line");
            assert!(
                sbom["components"]
                    .as_array()
                    .expect("components array")
                    .iter()
                    .any(|component| component["name"] == "is-positive"),
                "split SBOM should include dependencies from its project lockfile",
            );
            sbom["metadata"]["component"]["name"].as_str().expect("root component name").to_string()
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, ["project-a", "project-b"]);

    let filtered = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");
    assert!(
        filtered.status.success(),
        "filtered SBOM failed: {}",
        String::from_utf8_lossy(&filtered.stderr),
    );
    let sbom: serde_json::Value =
        serde_json::from_slice(&filtered.stdout).expect("parse filtered SBOM");
    assert_eq!(sbom["metadata"]["component"]["name"], "project-a");
    assert!(
        sbom["components"]
            .as_array()
            .expect("components array")
            .iter()
            .any(|component| component["name"] == "is-positive"),
        "filtered SBOM should include dependencies from the selected project's lockfile",
    );
}

#[test]
fn sbom_rejects_conflicting_entries_from_dedicated_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile =
        fs::read_to_string(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        let project_lockfile = if name == "project-b" {
            lockfile.replace(&["sha512-8N", "D1"].concat(), "sha512-different")
        } else {
            lockfile.clone()
        };
        fs::write(project_dir.join("pnpm-lock.yaml"), project_lockfile)
            .expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run split pacquet sbom");

    assert!(!output.status.success(), "conflicting lockfile entries must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES"), "stderr: {stderr}");
    let compact_stderr: String = stderr.replace('│', "").split_whitespace().collect();
    assert!(compact_stderr.contains("is-positive@3.1.0"), "stderr: {stderr}");
}

#[test]
fn sbom_merges_snapshot_optionality_from_dedicated_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile =
        fs::read_to_string(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    let lockfile = lockfile.replace(
        "    engines: {node: '>=0.10.0'}",
        "    engines: {node: '>=0.10.0'}\n    os: [unsupported-test-os]",
    );
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        let project_lockfile = if name == "project-b" {
            let project_lockfile = lockfile.replace(
                "  is-positive@3.1.0:\n    dev: false",
                "  is-positive@3.1.0:\n    optional: true\n    dev: false",
            );
            assert_ne!(project_lockfile, lockfile, "mark project-b's snapshot as optional");
            project_lockfile
        } else {
            lockfile.clone()
        };
        fs::write(project_dir.join("pnpm-lock.yaml"), project_lockfile)
            .expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let output = pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--split"])
        .output()
        .expect("run split pacquet sbom");

    assert!(
        output.status.success(),
        "derived snapshot optionality should merge: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let sboms: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .expect("SBOM output is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse split CycloneDX output"))
        .collect();
    assert_eq!(sboms.len(), 2);
    for sbom in sboms {
        let components = sbom["components"].as_array().expect("components array");
        assert!(
            components.iter().any(|component| component["name"] == "is-positive"),
            "a required platform-incompatible snapshot must remain in the SBOM",
        );
    }
}

#[test]
fn filtered_sbom_reads_reachable_workspace_project_lockfiles() {
    let tmp = dedicated_workspace_with_reachable_project();

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");

    assert!(
        output.status.success(),
        "filtered SBOM failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let sbom: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse filtered SBOM");
    let component_names: HashSet<&str> = sbom["components"]
        .as_array()
        .expect("components array")
        .iter()
        .filter_map(|component| component["name"].as_str())
        .collect();
    assert!(component_names.contains("project-b"));
    assert!(component_names.contains("is-positive"));
}

#[test]
fn filtered_sbom_rejects_a_reachable_project_without_a_dedicated_lockfile() {
    let tmp = dedicated_workspace_with_reachable_project();
    fs::remove_file(tmp.path().join("packages/project-b/pnpm-lock.yaml"))
        .expect("remove reachable project lockfile");

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");

    assert!(!output.status.success(), "an incomplete workspace graph must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_MISSING_IMPORTERS"), "stderr: {stderr}");
    assert!(stderr.contains("packages/project-b"), "stderr: {stderr}");
}

#[test]
fn sbom_exclude_peers_workspace_sub_packages() {
    let tmp = copy_fixture("with-peer-workspace");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(components.iter().any(|comp| comp["name"] == "is-positive"));
    assert!(
        !components.iter().any(|comp| comp["name"] == "is-odd"),
        "peer in sub-package should be excluded",
    );
}

#[test]
fn sbom_workspace_link_deps_as_components() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let names: Vec<&str> = components.iter().filter_map(|comp| comp["name"].as_str()).collect();
    assert!(names.contains(&"is-positive"), "registry dep should be included");
    assert!(names.contains(&"is-negative"), "registry dep from app-b should be included");
    assert!(names.contains(&"shared-lib"), "workspace link dep should be included as component");
    assert!(names.contains(&"is-odd"), "transitive dep of workspace link should be included");
}

#[test]
fn sbom_workspace_split_produces_multiple_lines() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        lines.len() >= 3,
        "workspace with 4 importers should produce at least 3 NDJSON lines (root may be empty), got {}",
        lines.len(),
    );
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
        assert_eq!(parsed["bomFormat"], "CycloneDX");
    }
}

#[test]
fn sbom_workspace_split_out_writes_files() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let out_pattern = tmp.path().join("out/%s.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let out_dir = tmp.path().join("out");
    let files: Vec<String> = fs::read_dir(&out_dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect();
    assert!(files.len() >= 3, "should write files for workspace packages, got {files:?}");
}

#[test]
fn sbom_workspace_split_out_percent_v() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let out_pattern = tmp.path().join("out/%s-%v.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let out_dir = tmp.path().join("out");
    let files: Vec<String> = fs::read_dir(&out_dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect();
    assert!(
        files.iter().any(|file| file.contains("1.0.0")),
        "filenames should contain version: {files:?}",
    );
}

#[test]
fn sbom_workspace_filter_selects_importer() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output = pacquet(
        tmp.path(),
        ["-F", "app-a", "sbom", "--sbom-format", "cyclonedx", "--lockfile-only"],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom with filter failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON output");
    let components = parsed["components"].as_array().expect("components");
    let names: Vec<&str> = components.iter().filter_map(|comp| comp["name"].as_str()).collect();
    assert!(names.contains(&"is-positive"), "app-a dep should be included");
    assert!(!names.contains(&"is-negative"), "app-b dep should be excluded by filter");
}

#[test]
fn sbom_workspace_link_dep_has_metadata() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let shared_lib = components.iter().find(|comp| comp["name"] == "shared-lib");
    assert!(shared_lib.is_some(), "shared-lib should be a component");
    let shared_lib = shared_lib.unwrap();
    assert_eq!(shared_lib["version"], "0.1.0");
    assert_eq!(shared_lib["purl"], "pkg:npm/shared-lib@0.1.0");
}

#[test]
fn sbom_workspace_spdx_link_deps() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let packages = parsed["packages"].as_array().expect("packages");
    assert!(
        packages.iter().any(|pkg| pkg["name"] == "shared-lib"),
        "shared-lib should be in SPDX packages",
    );
}

#[test]
fn sbom_workspace_split_each_line_has_correct_root() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let boms: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("valid JSON"))
        .collect();
    let root_names: Vec<&str> =
        boms.iter().filter_map(|bom| bom["metadata"]["component"]["name"].as_str()).collect();
    assert!(root_names.contains(&"app-a"), "split should include app-a");
    assert!(root_names.contains(&"app-b"), "split should include app-b");
}

#[test]
fn sbom_workspace_split_from_member_anchors_importers_at_workspace_root() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let member = tmp.path().join("app-a");
    let output =
        pacquet(&member, ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet from workspace member");

    assert!(
        output.status.success(),
        "member sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let boms = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).expect("valid JSON"))
        .collect::<Vec<_>>();
    let root_names = boms
        .iter()
        .filter_map(|bom| bom["metadata"]["component"]["name"].as_str())
        .collect::<Vec<_>>();

    assert!(root_names.contains(&"app-a"), "split should include app-a: {root_names:?}");
    assert!(root_names.contains(&"app-b"), "split should include app-b: {root_names:?}");
    assert!(root_names.contains(&"shared-lib"), "split should include shared-lib: {root_names:?}");
}

#[test]
fn sbom_workspace_from_member_uses_the_workspace_root_component() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(&tmp.path().join("app-a"), "cyclonedx", &[]);

    assert_eq!(parsed["metadata"]["component"]["name"], "workspace-sbom-root");
}

/// `--filter <pkg>...` walks every dependency edge, so a workspace
/// project reachable only through `devDependencies` is covered too.
#[test]
fn sbom_filter_selects_dev_dependency_projects() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split", "--filter", "app..."],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(split_root_names(&String::from_utf8_lossy(&output.stdout)), ["app", "dev-lib"]);
}

/// `--filter-prod <pkg>...` walks production dependencies only, so
/// `dev-lib` — a `devDependencies`-only workspace dependency of `app` —
/// is left out.
#[test]
fn sbom_filter_prod_follows_production_deps_only() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--filter-prod",
            "app...",
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(split_root_names(&String::from_utf8_lossy(&output.stdout)), ["app"]);
}

/// Selectors that match no workspace project skip the command entirely:
/// pnpm prints the notice and writes no SBOM, exiting zero.
#[test]
fn sbom_filter_matching_nothing_writes_no_sbom() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "no-such-package"],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success(), "no match alone must not fail the run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("No projects matched the filters in"), "stdout:\n{stdout}");
    assert!(!stdout.contains("bomFormat"), "no SBOM should be written:\n{stdout}");
}

/// `--workspace-root` narrows the SBOM to the root project, even though
/// the workspace package patterns don't name it — the root project is
/// always part of the workspace, so the `{<workspace-root>}` selector the
/// flag adds finds it.
#[test]
fn sbom_workspace_root_selects_only_the_root() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--workspace-root"],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON output");
    assert_eq!(parsed["metadata"]["component"]["name"], "workspace-sbom-filter-prod-root");
    assert!(
        parsed["components"].as_array().expect("components").is_empty(),
        "the root project has no dependencies, so nothing from app / dev-lib may leak in: {}",
        parsed["components"],
    );
}

/// A lockfile with no importer for a selected project is out of date, and
/// walking what is left would under-report the selection's dependencies —
/// so the run fails instead of writing that SBOM.
#[test]
fn sbom_fails_when_the_lockfile_has_no_importer_for_a_selected_project() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let added = tmp.path().join("newpkg");
    fs::create_dir_all(&added).expect("create the added package dir");
    fs::write(added.join("package.json"), r#"{ "name": "newpkg", "version": "1.0.0" }"#)
        .expect("write the added package.json");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - app\n  - dev-lib\n  - newpkg\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "newpkg"],
    )
    .output()
    .expect("run pacquet");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_MISSING_IMPORTERS"), "stderr:\n{stderr}");
    assert!(stderr.contains("newpkg"), "the error should name the missing project:\n{stderr}");
    assert!(!stdout.contains("bomFormat"), "no SBOM may be written for an out-of-date lockfile");
}
