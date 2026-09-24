use super::{
    _utils::{assert_success, ndjson_records, pacquet_in},
    workspace_yaml::{allow_builds, append_workspace_yaml_key, set_strict_dep_builds},
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pipe_trait::Pipe;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

/// `packageNames` from the run's `pnpm:ignored-scripts` NDJSON event.
/// The build phase emits the event exactly once per install, with an
/// empty list when nothing was ignored.
fn ignored_scripts_package_names(output: &std::process::Output) -> Vec<String> {
    let events: Vec<Vec<String>> = ndjson_records(output)
        .into_iter()
        .filter(|record| {
            record.get("name").and_then(serde_json::Value::as_str) == Some("pnpm:ignored-scripts")
        })
        .map(|record| {
            record["packageNames"]
                .as_array()
                .expect("packageNames is an array")
                .iter()
                .map(|name| {
                    name.as_str()
                        .expect("package name is a string")
                        .to_string()
                })
                .collect()
        })
        .collect();
    assert_eq!(events.len(), 1, "expected exactly one pnpm:ignored-scripts event; got {events:?}");
    events
        .into_iter()
        .next()
        .expect("asserted a single event above")
}

#[test]
fn run_pre_and_postinstall_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );

    eprintln!("Checking generated-by-prepare.js does NOT exist...");
    assert!(
        !pkg_dir.join("generated-by-prepare.js").exists(),
        "prepare should not run for registry packages",
    );

    eprintln!("Checking generated-by-preinstall.js exists...");
    assert!(pkg_dir.join("generated-by-preinstall.js").exists());

    eprintln!("Checking generated-by-postinstall.js exists...");
    assert!(pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

#[test]
fn run_install_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    set_strict_dep_builds(&workspace, false);
    allow_builds(&workspace, &[("@pnpm.e2e/install-script-example", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
    );

    eprintln!("Checking generated-by-install.js exists...");
    assert!(pkg_dir.join("generated-by-install.js").exists());

    drop((root, mock_instance));
}

#[test]
fn lifecycle_scripts_run_in_dependency_order() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/with-postinstall-a": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    allow_builds(
        &workspace,
        &[("@pnpm.e2e/with-postinstall-a", true), ("@pnpm.e2e/with-postinstall-b", true)],
    );

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    let output_a: serde_json::Value = virtual_store
        .join(
            "@pnpm.e2e+with-postinstall-a@1.0.0\
                 /node_modules/@pnpm.e2e/with-postinstall-a/output.json",
        )
        .pipe_ref(fs::read_to_string)
        .expect("read output A")
        .pipe_ref(|s| serde_json::from_str(s))
        .expect("parse output A");
    let output_b: serde_json::Value = virtual_store
        .join(
            "@pnpm.e2e+with-postinstall-b@1.0.0\
                 /node_modules/@pnpm.e2e/with-postinstall-b/output.json",
        )
        .pipe_ref(fs::read_to_string)
        .expect("read output B")
        .pipe_ref(|s| serde_json::from_str(s))
        .expect("parse output B");

    // `json-append` stores the `Number(new Date())` timestamp as a
    // JSON string; mirror upstream's `+value` coercion.
    let timestamp = |value: &serde_json::Value| -> u64 {
        value[0]
            .as_str()
            .expect("timestamp string")
            .parse()
            .expect("parse timestamp")
    };
    let ts_b = timestamp(&output_b);
    let ts_a = timestamp(&output_a);
    eprintln!("Checking B ran before A (B={ts_b}, A={ts_a})...");
    assert!(ts_b < ts_a, "dependency B should run before dependent A");

    drop((root, mock_instance));
}

#[test]
fn lifecycle_scripts_run_before_linking_bins() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/generated-bins": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/generated-bins", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let node_modules = workspace.join("node_modules");

    eprintln!("Checking generated bins are executable...");
    #[cfg(unix)]
    {
        use pnpm_testing_utils::fs::is_path_executable;
        assert!(is_path_executable(&node_modules.join(".bin/cmd1")));
        assert!(is_path_executable(&node_modules.join(".bin/cmd2")));
    }

    eprintln!("Deleting node_modules for frozen reinstall...");
    fs::remove_dir_all(&node_modules).expect("remove node_modules");

    eprintln!("Running pacquet install --frozen-lockfile...");
    let CommandTempCwd {
        pacquet: frozen_pacquet,
        root: frozen_root,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    frozen_pacquet
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    eprintln!("Checking generated bins are executable after frozen reinstall...");
    #[cfg(unix)]
    {
        use pnpm_testing_utils::fs::is_path_executable;
        assert!(is_path_executable(&node_modules.join(".bin/cmd1")));
        assert!(is_path_executable(&node_modules.join(".bin/cmd2")));
    }

    drop((root, mock_instance, frozen_root));
}

/// A package's lifecycle scripts run with its own `node_modules/.bin` and the
/// project's on `PATH`, so a shim of a bin the `preinstall` has not created
/// yet would shadow the command the script runs (pnpm/pnpm#15501). The
/// fixture's `preinstall` fails if any shim of its bin is on `PATH`.
#[test]
fn own_bin_is_not_on_path_before_preinstall_creates_it() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/own-bin-created-by-preinstall": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/own-bin-created-by-preinstall", true)]);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let bin = workspace.join("node_modules/.bin/own-bin-created-by-preinstall");
    assert!(bin.exists(), "the project gets the bin once preinstall created it");

    drop((root, mock_instance));
}

#[test]
fn hoisting_tolerates_bins_created_by_a_later_lifecycle_stage() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/has-generated-bins-as-dep": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read workspace manifest");
    yaml.push_str("hoistPattern:\n  - '*'\n");
    fs::write(&yaml_path, yaml).expect("write workspace manifest");
    allow_builds(
        &workspace,
        &[("@pnpm.e2e/has-generated-bins-as-dep", true), ("@pnpm.e2e/generated-bins", true)],
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, mock_instance));
}

#[test]
fn bins_linked_even_if_scripts_ignored() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/peer-with-bin": "1.0.0",
            "@pnpm.e2e/pkg-with-peer-having-bin": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    // No `allowBuilds`: scripts are intentionally ignored. Disable
    // strict mode so the install completes and the bins can still be
    // inspected (strict would fail with `ERR_PNPM_IGNORED_BUILDS`).
    set_strict_dep_builds(&workspace, false);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let node_modules = workspace.join("node_modules");

    eprintln!("Checking bins are linked...");
    #[cfg(unix)]
    {
        use pnpm_testing_utils::fs::is_path_executable;
        assert!(is_path_executable(&node_modules.join(".bin/peer-with-bin")));
    }

    let scripts_pkg_dir = node_modules.join(
        ".pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );

    eprintln!("Checking package.json exists but generated files do not...");
    assert!(scripts_pkg_dir.join("package.json").exists());
    assert!(
        !scripts_pkg_dir.join("generated-by-preinstall.js").exists(),
        "scripts should not have run with ignoreScripts",
    );

    eprintln!("Deleting node_modules for frozen reinstall...");
    fs::remove_dir_all(&node_modules).expect("remove node_modules");

    eprintln!("Running pacquet install --frozen-lockfile...");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    eprintln!("Checking bins are linked after frozen reinstall...");
    #[cfg(unix)]
    {
        use pnpm_testing_utils::fs::is_path_executable;
        assert!(is_path_executable(&node_modules.join(".bin/peer-with-bin")));
    }

    eprintln!("Checking scripts stayed ignored after frozen reinstall...");
    assert!(scripts_pkg_dir.join("package.json").exists());
    assert!(
        !scripts_pkg_dir.join("generated-by-preinstall.js").exists(),
        "scripts should not have run on the frozen reinstall either",
    );

    drop((root, mock_instance));
}

#[test]
fn selectively_ignore_scripts_by_allow_builds() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json with allowBuilds...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    set_strict_dep_builds(&workspace, false);
    allow_builds(&workspace, &[("@pnpm.e2e/install-script-example", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");

    let denied_pkg = virtual_store.join(
        "@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    eprintln!("Checking denied package did NOT run scripts...");
    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());

    let allowed_pkg = virtual_store.join(
        "@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
    );
    eprintln!("Checking allowed package DID run scripts...");
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    eprintln!("Deleting node_modules for frozen reinstall...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    eprintln!("Checking denied package still did NOT run scripts after frozen reinstall...");
    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());
    eprintln!("Checking allowed package DID run scripts after frozen reinstall...");
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    drop((root, mock_instance));
}

#[test]
fn selectively_allow_scripts_by_allow_builds() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json with allowBuilds...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    set_strict_dep_builds(&workspace, false);
    allow_builds(&workspace, &[("@pnpm.e2e/install-script-example", true)]);

    eprintln!("Running pacquet install...");
    let output = pacquet
        .with_args(["--reporter=ndjson", "install"])
        .output()
        .expect("run pacquet");
    assert_success(&output);

    let virtual_store = workspace.join("node_modules/.pnpm");
    let node_modules = workspace.join("node_modules");

    let denied_pkg = virtual_store.join(
        "@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    eprintln!("Checking denied package did NOT run scripts...");
    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());

    let allowed_pkg = virtual_store.join(
        "@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
    );
    eprintln!("Checking allowed package DID run scripts...");
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    eprintln!("Checking pnpm:ignored-scripts lists the unapproved package...");
    assert_eq!(
        ignored_scripts_package_names(&output),
        ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );

    eprintln!("Re-running install with explicit denial of pre-and-postinstall-scripts-example...");
    fs::remove_dir_all(&node_modules).expect("remove node_modules");
    allow_builds(
        &workspace,
        &[
            ("@pnpm.e2e/install-script-example", true),
            ("@pnpm.e2e/pre-and-postinstall-scripts-example", false),
        ],
    );

    let frozen_output = pacquet_in(&workspace)
        .with_args(["--reporter=ndjson", "install", "--frozen-lockfile"])
        .output()
        .expect("run pacquet install --frozen-lockfile");
    assert_success(&frozen_output);

    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    eprintln!("Checking pnpm:ignored-scripts is empty under explicit denial...");
    // Explicit denial moves the package from "ignored" to "silently
    // skipped", so the event carries no package names this time.
    assert_eq!(ignored_scripts_package_names(&frozen_output), Vec::<String>::new());

    drop((root, mock_instance));
}

#[test]
fn selectively_allow_scripts_by_allow_builds_exact_versions() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json with exact-version allowBuilds...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    set_strict_dep_builds(&workspace, false);
    allow_builds(&workspace, &[("@pnpm.e2e/install-script-example@1.0.0", true)]);

    eprintln!("Running pacquet install...");
    let output = pacquet
        .with_args(["--reporter=ndjson", "install"])
        .output()
        .expect("run pacquet");
    assert_success(&output);

    let virtual_store = workspace.join("node_modules/.pnpm");

    let denied_pkg = virtual_store.join(
        "@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    eprintln!("Checking denied package did NOT run scripts...");
    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());

    let allowed_pkg = virtual_store.join(
        "@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
    );
    eprintln!("Checking allowed package DID run scripts...");
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    eprintln!("Checking pnpm:ignored-scripts lists the unapproved package...");
    assert_eq!(
        ignored_scripts_package_names(&output),
        ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );

    eprintln!("Re-running install with explicit denial of pre-and-postinstall-scripts-example...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    allow_builds(
        &workspace,
        &[
            ("@pnpm.e2e/install-script-example@1.0.0", true),
            ("@pnpm.e2e/pre-and-postinstall-scripts-example", false),
        ],
    );

    let frozen_output = pacquet_in(&workspace)
        .with_args(["--reporter=ndjson", "install", "--frozen-lockfile"])
        .output()
        .expect("run pacquet install --frozen-lockfile");
    assert_success(&frozen_output);

    assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
    assert!(!denied_pkg.join("generated-by-postinstall.js").exists());
    assert!(allowed_pkg.join("generated-by-install.js").exists());

    eprintln!("Checking pnpm:ignored-scripts is empty under explicit denial...");
    assert_eq!(ignored_scripts_package_names(&frozen_output), Vec::<String>::new());

    drop((root, mock_instance));
}

#[test]
fn lifecycle_scripts_run_after_linking_root_deps() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "is-positive": "1.0.0",
            "@pnpm.e2e/postinstall-requires-is-positive": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/postinstall-requires-is-positive", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    eprintln!("Deleting node_modules for frozen reinstall...");
    let node_modules = workspace.join("node_modules");
    fs::remove_dir_all(&node_modules).expect("remove node_modules");

    eprintln!("Running pacquet install --frozen-lockfile...");
    let CommandTempCwd {
        pacquet: frozen_pacquet,
        root: frozen_root,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    frozen_pacquet
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, mock_instance, frozen_root));
}

#[test]
fn rebuild_after_allow_builds_changes() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json with partial allowBuilds...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    set_strict_dep_builds(&workspace, false);
    allow_builds(&workspace, &[("@pnpm.e2e/install-script-example", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");

    let install_pkg = virtual_store.join(
        "@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
    );
    eprintln!("Checking allowed package ran scripts...");
    assert!(install_pkg.join("generated-by-install.js").exists());

    let scripts_pkg = virtual_store.join(
        "@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    eprintln!("Checking denied package did NOT run scripts...");
    assert!(!scripts_pkg.join("generated-by-preinstall.js").exists());
    assert!(!scripts_pkg.join("generated-by-postinstall.js").exists());

    eprintln!("Updating allowBuilds and running frozen reinstall...");
    allow_builds(
        &workspace,
        &[
            ("@pnpm.e2e/install-script-example", true),
            ("@pnpm.e2e/pre-and-postinstall-scripts-example", true),
        ],
    );

    let CommandTempCwd {
        pacquet: frozen_pacquet,
        root: frozen_root,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    frozen_pacquet
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    eprintln!("Checking all scripts ran after allowBuilds change...");
    assert!(install_pkg.join("generated-by-install.js").exists());
    assert!(scripts_pkg.join("generated-by-preinstall.js").exists());
    assert!(scripts_pkg.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance, frozen_root));
}

#[test]
fn headless_run_pre_postinstall_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    eprintln!("Running pacquet install...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    eprintln!("Checking generated-by-preinstall.js exists...");
    assert!(pkg_dir.join("generated-by-preinstall.js").exists());
    eprintln!("Checking generated-by-postinstall.js exists...");
    assert!(pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

mod strict;
