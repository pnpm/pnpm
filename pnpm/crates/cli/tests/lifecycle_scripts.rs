pub mod _utils;

/// Helpers for editing the `pnpm-workspace.yaml` that
/// [`CommandTempCwd::add_mocked_registry`] wrote. Both edit in place
/// rather than replacing the file: the harness's `storeDir`, `cacheDir`,
/// and `enableGlobalVirtualStore: false` keys have to survive, or the
/// test installs into the real store and the global virtual store moves
/// every package out of `node_modules/.pnpm`.
mod workspace_yaml {
    use std::{fmt::Write as _, fs, path::Path};

    /// Set `strictDepBuilds`. Tests that intentionally leave builds
    /// ignored and then inspect the filesystem set it to `false` so the
    /// install completes instead of failing with
    /// `ERR_PNPM_IGNORED_BUILDS` (the default). Must be called before
    /// [`allow_builds`] so its line survives the `allowBuilds:`
    /// truncation on re-calls.
    pub fn set_strict_dep_builds(workspace: &Path, strict: bool) {
        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
        if !yaml.is_empty() && !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        writeln!(yaml, "strictDepBuilds: {strict}").expect("format strictDepBuilds");
        fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    }

    /// Append a top-level `key: value` line. Only valid for keys the
    /// harness never writes; the assert fails loudly if that changes.
    pub fn append_workspace_yaml_key(workspace: &Path, key: &str, value: impl std::fmt::Display) {
        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
        let key_prefix = format!("{key}:");
        assert!(
            !yaml.lines().any(|line| line.starts_with(&key_prefix)),
            "pnpm-workspace.yaml already has a `{key}:` key",
        );
        if !yaml.is_empty() && !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        writeln!(yaml, "{key}: {value}").expect("format workspace key");
        fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    }

    /// Set the `allowBuilds:` block, replacing any block a previous
    /// phase wrote. pnpm v11 (and pacquet) read build approval from
    /// `pnpm-workspace.yaml`, not from `package.json#pnpm` — this
    /// mirrors upstream tests passing `allowBuilds` through
    /// `testDefaults`. An empty `entries` withdraws every approval.
    pub fn allow_builds(workspace: &Path, entries: &[(&str, bool)]) {
        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
        if let Some(idx) = yaml.find("allowBuilds:") {
            yaml.truncate(idx);
        }
        if !yaml.is_empty() && !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        if entries.is_empty() {
            fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
            return;
        }
        yaml.push_str("allowBuilds:\n");
        for (spec, value) in entries {
            writeln!(yaml, "  '{spec}': {value}").expect("format allowBuilds entry");
        }
        fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    }
}

mod dependency_build_scripts {
    use super::{
        _utils::{assert_success, ndjson_records, pacquet_in},
        workspace_yaml::{allow_builds, append_workspace_yaml_key, set_strict_dep_builds},
    };
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
    use pipe_trait::Pipe;
    use std::{fs, path::Path, process::Command};

    /// `packageNames` from the run's `pnpm:ignored-scripts` NDJSON event.
    /// The build phase emits the event exactly once per install, with an
    /// empty list when nothing was ignored.
    fn ignored_scripts_package_names(output: &std::process::Output) -> Vec<String> {
        let events: Vec<Vec<String>> = ndjson_records(output)
            .into_iter()
            .filter(|record| {
                record.get("name").and_then(serde_json::Value::as_str)
                    == Some("pnpm:ignored-scripts")
            })
            .map(|record| {
                record["packageNames"]
                    .as_array()
                    .expect("packageNames is an array")
                    .iter()
                    .map(|name| name.as_str().expect("package name is a string").to_string())
                    .collect()
            })
            .collect();
        assert_eq!(
            events.len(),
            1,
            "expected exactly one pnpm:ignored-scripts event; got {events:?}",
        );
        events.into_iter().next().expect("asserted a single event above")
    }

    #[test]
    fn run_pre_and_postinstall_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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
            value[0].as_str().expect("timestamp string").parse().expect("parse timestamp")
        };
        let ts_b = timestamp(&output_b);
        let ts_a = timestamp(&output_a);
        eprintln!("Checking B ran before A (B={ts_b}, A={ts_a})...");
        assert!(ts_b < ts_a, "dependency B should run before dependent A");

        drop((root, mock_instance));
    }

    #[test]
    fn lifecycle_scripts_run_before_linking_bins() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

        let node_modules = workspace.join("node_modules");

        eprintln!("Checking generated bins are executable...");
        #[cfg(unix)]
        {
            use pacquet_testing_utils::fs::is_path_executable;
            assert!(is_path_executable(&node_modules.join(".bin/cmd1")));
            assert!(is_path_executable(&node_modules.join(".bin/cmd2")));
        }

        eprintln!("Deleting node_modules for frozen reinstall...");
        fs::remove_dir_all(&node_modules).expect("remove node_modules");

        eprintln!("Running pacquet install --frozen-lockfile...");
        let CommandTempCwd { pacquet: frozen_pacquet, root: frozen_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        frozen_pacquet
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        eprintln!("Checking generated bins are executable after frozen reinstall...");
        #[cfg(unix)]
        {
            use pacquet_testing_utils::fs::is_path_executable;
            assert!(is_path_executable(&node_modules.join(".bin/cmd1")));
            assert!(is_path_executable(&node_modules.join(".bin/cmd2")));
        }

        drop((root, mock_instance, frozen_root));
    }

    #[test]
    fn hoisting_tolerates_bins_created_by_a_later_lifecycle_stage() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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

        pacquet.with_arg("install").assert().success();
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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

        let node_modules = workspace.join("node_modules");

        eprintln!("Checking bins are linked...");
        #[cfg(unix)]
        {
            use pacquet_testing_utils::fs::is_path_executable;
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
        pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

        eprintln!("Checking bins are linked after frozen reinstall...");
        #[cfg(unix)]
        {
            use pacquet_testing_utils::fs::is_path_executable;
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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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

        pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

        eprintln!("Checking denied package still did NOT run scripts after frozen reinstall...");
        assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
        assert!(!denied_pkg.join("generated-by-postinstall.js").exists());
        eprintln!("Checking allowed package DID run scripts after frozen reinstall...");
        assert!(allowed_pkg.join("generated-by-install.js").exists());

        drop((root, mock_instance));
    }

    #[test]
    fn selectively_allow_scripts_by_allow_builds() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        let output =
            pacquet.with_args(["--reporter=ndjson", "install"]).output().expect("run pacquet");
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

        eprintln!(
            "Re-running install with explicit denial of pre-and-postinstall-scripts-example...",
        );
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
        assert_eq!(ignored_scripts_package_names(&frozen_output), Vec::<String>::new());

        drop((root, mock_instance));
    }

    #[test]
    fn selectively_allow_scripts_by_allow_builds_exact_versions() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        let output =
            pacquet.with_args(["--reporter=ndjson", "install"]).output().expect("run pacquet");
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

        eprintln!(
            "Re-running install with explicit denial of pre-and-postinstall-scripts-example...",
        );
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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

        eprintln!("Deleting node_modules for frozen reinstall...");
        let node_modules = workspace.join("node_modules");
        fs::remove_dir_all(&node_modules).expect("remove node_modules");

        eprintln!("Running pacquet install --frozen-lockfile...");
        let CommandTempCwd { pacquet: frozen_pacquet, root: frozen_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        frozen_pacquet
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        drop((root, mock_instance, frozen_root));
    }

    #[test]
    fn rebuild_after_allow_builds_changes() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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

        let CommandTempCwd { pacquet: frozen_pacquet, root: frozen_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
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
        pacquet.with_arg("install").assert().success();

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

    /// Regression test for the user-reported gap: `pacquet add <pkg>`
    /// takes the fresh-lockfile path, which never ran the build phase —
    /// so a blocked dependency build script was silently ignored, and
    /// the install exited 0, unlike `pnpm add`. Under the default
    /// `strictDepBuilds`, the install now fails with
    /// `ERR_PNPM_IGNORED_BUILDS` after adding the dependency.
    #[test]
    fn add_fails_under_strict_dep_builds_when_a_build_is_ignored() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");

        // No `allowBuilds` and `strictDepBuilds` defaults to true, so the
        // blocked build must fail the install with a non-zero exit, like
        // `pnpm add`.
        let output = pacquet
            .with_args(["add", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"])
            .output()
            .expect("run pacquet add");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("pacquet add stdout:\n{stdout}\nstderr:\n{stderr}");
        assert!(!output.status.success(), "strict add with an ignored build must exit non-zero");
        let combined = format!("{stdout}{stderr}");
        assert!(
            combined.contains("ERR_PNPM_IGNORED_BUILDS")
                && combined.contains("Ignored build scripts")
                && combined.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
            "expected ERR_PNPM_IGNORED_BUILDS naming the package; got:\n{combined}",
        );

        // The dependency was still added and materialized; only its
        // blocked scripts did not run. Mirrors pnpm, which writes the
        // artifacts before failing.
        let pkg_dir = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
        );
        assert!(pkg_dir.join("package.json").exists(), "dependency should be materialized");
        assert!(!pkg_dir.join("generated-by-preinstall.js").exists());
        assert!(!pkg_dir.join("generated-by-postinstall.js").exists());

        drop((root, mock_instance));
    }

    /// `pacquet install --ignore-scripts` must exit 0 even under the
    /// default `strictDepBuilds`, with no `allowBuilds`: `--ignore-scripts`
    /// suppresses every dependency build, so nothing is reported as an
    /// ignored build and the strict gate never fires. The dependency is
    /// still materialized, but its lifecycle scripts do not run. Mirrors
    /// pnpm's `--ignore-scripts`, which leaves `ignoredBuilds` empty.
    #[test]
    fn install_ignore_scripts_does_not_fail_under_strict_dep_builds() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let manifest_path = workspace.join("package.json");
        let package_json = serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        // No `allowBuilds` and `strictDepBuilds` defaults to true: a plain
        // `pacquet install` would fail with `ERR_PNPM_IGNORED_BUILDS`.
        // `--ignore-scripts` must make it exit 0 instead.
        let output = pacquet
            .with_args(["install", "--ignore-scripts"])
            .output()
            .expect("run pacquet install --ignore-scripts");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("pacquet install --ignore-scripts stdout:\n{stdout}\nstderr:\n{stderr}");
        assert!(output.status.success(), "install --ignore-scripts must exit zero");
        assert!(
            !format!("{stdout}{stderr}").contains("ERR_PNPM_IGNORED_BUILDS"),
            "--ignore-scripts must not report ignored builds",
        );

        let pkg_dir = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
        );
        assert!(pkg_dir.join("package.json").exists(), "dependency should be materialized");
        assert!(!pkg_dir.join("generated-by-preinstall.js").exists());
        assert!(!pkg_dir.join("generated-by-postinstall.js").exists());

        drop((root, mock_instance));
    }

    /// With `strictDepBuilds: false`, an ignored build is a non-fatal
    /// warning printed to stdout and the install exits 0 — the
    /// ignored-build-scripts warning box.
    #[test]
    fn add_warns_without_strict_dep_builds_when_a_build_is_ignored() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");
        set_strict_dep_builds(&workspace, false);

        let output = pacquet
            .with_args(["add", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"])
            .output()
            .expect("run pacquet add");
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("pacquet add stdout:\n{stdout}");
        assert!(output.status.success(), "non-strict add must exit zero");
        assert!(
            stdout.contains("Ignored build scripts")
                && stdout.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
            "expected an ignored-build-scripts warning naming the package; got:\n{stdout}",
        );

        let pkg_dir = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
        );
        assert!(!pkg_dir.join("generated-by-preinstall.js").exists());

        drop((root, mock_instance));
    }

    /// `strictDepBuilds` must stay enforced across reruns: after a strict
    /// install fails with `ERR_PNPM_IGNORED_BUILDS`, a warm rerun must
    /// fail again rather than short-circuit to exit 0 via an up-to-date
    /// fast path — otherwise rerunning install would bypass the gate.
    #[test]
    fn strict_install_keeps_failing_on_warm_rerun() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        // First install (strict default, no `allowBuilds`): fails, but
        // still materializes the dep and records the ignored build in
        // `.modules.yaml`.
        let first = pacquet.with_arg("install").output().expect("run pacquet install");
        assert!(!first.status.success(), "first strict install with an ignored build must fail");

        // Warm rerun: the lockfile and `.modules.yaml` are unchanged, so
        // the up-to-date fast path would normally exit 0 — but strict mode
        // must keep failing until the build is approved.
        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let rerun_out = rerun
            .with_current_dir(&workspace)
            .with_arg("install")
            .output()
            .expect("run pacquet install again");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&rerun_out.stdout),
            String::from_utf8_lossy(&rerun_out.stderr),
        );
        eprintln!("rerun output:\n{combined}");
        assert!(
            !rerun_out.status.success(),
            "a warm rerun must not bypass strictDepBuilds; got:\n{combined}",
        );
        assert!(
            combined.contains("ERR_PNPM_IGNORED_BUILDS"),
            "rerun should still report ERR_PNPM_IGNORED_BUILDS; got:\n{combined}",
        );

        drop((root, mock_instance, rerun_root));
    }

    /// A corrupt / unreadable `.modules.yaml` must not let a strict rerun
    /// short-circuit to exit 0: the up-to-date fast paths can't prove the
    /// absence of recorded ignored builds from an unparsable state file,
    /// so they conservatively fall through to the full install, which
    /// fails again with `ERR_PNPM_IGNORED_BUILDS`.
    #[test]
    fn strict_install_keeps_failing_with_unreadable_modules_yaml() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        let first = pacquet.with_arg("install").output().expect("run pacquet install");
        assert!(!first.status.success(), "first strict install with an ignored build must fail");

        // Corrupt the recorded state so it can't be parsed.
        fs::write(workspace.join("node_modules/.modules.yaml"), "}{ not: valid: yaml")
            .expect("corrupt .modules.yaml");

        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let rerun_out = rerun
            .with_current_dir(&workspace)
            .with_arg("install")
            .output()
            .expect("run pacquet install again");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&rerun_out.stdout),
            String::from_utf8_lossy(&rerun_out.stderr),
        );
        eprintln!("rerun output:\n{combined}");
        assert!(
            !rerun_out.status.success(),
            "a strict rerun with a corrupt .modules.yaml must not exit 0; got:\n{combined}",
        );

        drop((root, mock_instance, rerun_root));
    }

    /// TS: `the list of ignored builds is preserved after a repeat
    /// install` (`pnpm/test/install/lifecycleScripts.ts:245`). A repeat
    /// install re-reports every ignored build and leaves the recorded
    /// set intact — dropping an entry would make an approved-nothing
    /// rerun look clean.
    #[test]
    fn ignored_builds_are_preserved_after_a_repeat_install() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");

        // Both commands exit non-zero: under the default `strictDepBuilds`
        // an ignored build is `ERR_PNPM_IGNORED_BUILDS`, not a warning. The
        // add still materializes the packages and records them, and the
        // point under test is that the repeat install re-reports the same
        // set rather than a stale rerun looking clean.
        let add_out = pacquet
            .with_args([
                "add",
                "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
                "@pnpm.e2e/install-script-example@1.0.0",
                "--config.optimistic-repeat-install=false",
            ])
            .output()
            .expect("run pacquet add");
        assert!(
            !add_out.status.success(),
            "a strict add with ignored builds must fail; got:\n{}{}",
            String::from_utf8_lossy(&add_out.stdout),
            String::from_utf8_lossy(&add_out.stderr),
        );

        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let rerun_out = rerun
            .with_current_dir(&workspace)
            .with_args(["install", "--config.optimistic-repeat-install=false"])
            .output()
            .expect("run pacquet install again");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&rerun_out.stdout),
            String::from_utf8_lossy(&rerun_out.stderr),
        );
        eprintln!("repeat install output:\n{combined}");
        assert!(!rerun_out.status.success(), "the strict repeat install must keep failing");
        assert!(
            combined.contains("Ignored build scripts"),
            "the repeat install must report the ignored builds again; got:\n{combined}",
        );

        let recorded = read_ignored_builds(&workspace);
        assert_eq!(
            recorded,
            [
                "@pnpm.e2e/install-script-example@1.0.0",
                "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
            ],
        );

        drop((root, mock_instance, rerun_root));
    }

    /// TS: `strictDepBuilds fails for packages with cached side-effects`
    /// (`pnpm/test/install/lifecycleScripts.ts:380`,
    /// <https://github.com/pnpm/pnpm/issues/11035>). Revoking a build
    /// approval must fail the strict gate even though the store already
    /// holds that package's build output — the side-effects cache is an
    /// optimization, never an approval.
    #[test]
    fn strict_dep_builds_fails_for_packages_with_cached_side_effects() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        set_strict_dep_builds(&workspace, true);
        append_workspace_yaml_key(&workspace, "optimisticRepeatInstall", false);
        allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

        pacquet.with_arg("install").assert().success();
        let built_marker = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        );
        assert!(built_marker.exists(), "the approved build must run and populate the cache");

        allow_builds(&workspace, &[]);

        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let rerun_out = rerun
            .with_current_dir(&workspace)
            .with_arg("install")
            .output()
            .expect("run pacquet install after revoking approval");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&rerun_out.stdout),
            String::from_utf8_lossy(&rerun_out.stderr),
        );
        eprintln!("revoked-approval install output:\n{combined}");
        assert!(
            !rerun_out.status.success(),
            "a cached build must not satisfy the strict gate; got:\n{combined}",
        );
        assert!(
            combined.contains("Ignored build scripts"),
            "the revoked package must be reported as ignored; got:\n{combined}",
        );

        drop((root, mock_instance, rerun_root));
    }

    fn read_ignored_builds(workspace: &Path) -> Vec<String> {
        let mut recorded: Vec<String> = pacquet_modules_yaml::read_modules_manifest::<
            pacquet_modules_yaml::Host,
        >(&workspace.join("node_modules"))
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists")
        .ignored_builds
        .unwrap_or_default()
        .into_iter()
        .map(|dep_path| dep_path.as_str().to_string())
        .collect();
        recorded.sort();
        recorded
    }
}

/// `.modules.yaml`'s `pendingBuilds` — the record of builds
/// `--ignore-scripts` deferred, which `pacquet rebuild --pending`
/// later drains.
mod pending_builds {
    use super::workspace_yaml::allow_builds;
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pacquet_modules_yaml::{Host, read_modules_manifest};
    use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
    use std::{fs, path::Path};

    fn read_pending_builds(workspace: &Path) -> Vec<String> {
        read_modules_manifest::<Host>(&workspace.join("node_modules"))
            .expect("read .modules.yaml")
            .expect(".modules.yaml exists")
            .pending_builds
    }

    /// TS: `run pre/postinstall scripts` (`deps-restorer/test/index.ts:362`),
    /// the `ignoreScripts` tail. Ordering is pnpm's: projects first.
    #[test]
    fn ignore_scripts_records_the_project_and_its_deferred_dependency() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "scripts": { "install": r#"node -e """#, "postinstall": r#"node -e """# },
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

        assert_eq!(
            read_pending_builds(&workspace),
            [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
        );

        drop((root, mock_instance));
    }

    /// A project without install-time scripts of its own contributes no
    /// importer entry — only the deferred dependency builds are recorded.
    #[test]
    fn a_project_without_install_scripts_records_only_dependencies() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

        assert_eq!(
            read_pending_builds(&workspace),
            ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
        );

        drop((root, mock_instance));
    }

    /// An install that runs the build scripts owes nothing, so the list
    /// stays empty — this is what makes a populated list mean "deferred".
    #[test]
    fn an_install_that_runs_the_scripts_records_nothing() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

        pacquet.with_arg("install").assert().success();

        assert_eq!(read_pending_builds(&workspace), Vec::<String>::new());

        drop((root, mock_instance));
    }

    /// TS: `pendingBuilds gets updated if install removes packages`
    /// (`deps-installer/test/lockfile.ts:614`). Dropping one of two
    /// deferred dependencies shrinks the recorded list.
    #[test]
    fn removing_a_package_shrinks_the_list() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let manifest_path = workspace.join("package.json");
        let both = serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0",
            },
        });
        fs::write(&manifest_path, both.to_string()).expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
        let before = read_pending_builds(&workspace);
        assert_eq!(
            before,
            [
                "@pnpm.e2e/install-script-example@1.0.0",
                "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
            ],
        );

        let one = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(&manifest_path, one.to_string()).expect("rewrite package.json");

        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        rerun
            .with_current_dir(&workspace)
            .with_args(["install", "--ignore-scripts"])
            .assert()
            .success();

        let after = read_pending_builds(&workspace);
        assert_eq!(after, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);
        assert!(after.len() < before.len(), "removing a package must shrink {before:?}");

        drop((root, mock_instance, rerun_root));
    }

    /// `pnpm rebuild --pending` is what settles the debt: it runs both
    /// the deferred dependency builds and the project's own deferred
    /// install scripts, then leaves the list empty so a second run has
    /// nothing to do.
    #[test]
    fn rebuild_pending_runs_the_deferred_work_and_empties_the_list() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let marker = workspace.join("project-install-ran.txt");
        let package_json = serde_json::json!({
            "scripts": {
                "install": r#"node -e "require('fs').writeFileSync('project-install-ran.txt','ran')""#,
            },
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
        assert_eq!(
            read_pending_builds(&workspace),
            [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
        );
        assert!(!marker.exists(), "--ignore-scripts must defer the project's own script");

        let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        rebuild.with_current_dir(&workspace).with_args(["rebuild", "--pending"]).assert().success();

        assert!(marker.exists(), "the deferred project script must run");
        assert!(
            workspace
                .join(
                    "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
                     /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example\
                     /generated-by-postinstall.js"
                )
                .exists(),
            "the deferred dependency build must run",
        );
        assert_eq!(
            read_pending_builds(&workspace),
            Vec::<String>::new(),
            "a rebuild settles the debt it was asked to run",
        );

        drop((root, mock_instance, rebuild_root));
    }

    /// A `rebuild --pending` that the `allowBuilds` policy still blocks
    /// runs nothing, so it must leave the debt in place — dropping it
    /// would let a later `--pending` report success on a build that never
    /// ran.
    #[test]
    fn rebuild_pending_keeps_a_dependency_the_policy_still_blocks() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
        let pending = read_pending_builds(&workspace);
        assert_eq!(pending, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);

        // No `allowBuilds` entry, so `rebuild --pending` cannot build it.
        // Its exit code is beside the point — the assertion is that the
        // debt survives whether it exits 0 (warn) or 1 (strict).
        let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let _ = rebuild
            .with_current_dir(&workspace)
            .with_args(["rebuild", "--pending"])
            .output()
            .expect("run pacquet rebuild --pending");

        assert_eq!(
            read_pending_builds(&workspace),
            pending,
            "a build the policy blocked stays owed",
        );

        drop((root, mock_instance, rebuild_root));
    }

    /// A project's scripts run after `.modules.yaml` is written, so its
    /// entry can only be cleared once they have actually succeeded —
    /// otherwise a failing script would lose the debt it just failed to
    /// discharge.
    #[test]
    fn a_failed_project_script_keeps_its_pending_entry() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "scripts": { "install": r#"node -e "process.exit(1)""# },
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
        assert_eq!(
            read_pending_builds(&workspace),
            [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
        );

        let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let output = rebuild
            .with_current_dir(&workspace)
            .with_args(["rebuild", "--pending"])
            .output()
            .expect("run pacquet rebuild --pending");
        eprintln!(
            "rebuild --pending output:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(!output.status.success(), "a failing project script must fail the rebuild");

        assert_eq!(
            read_pending_builds(&workspace),
            ["."],
            "the dependency was rebuilt, the project was not",
        );

        drop((root, mock_instance, rebuild_root));
    }

    /// A build stays owed until something runs it: an install that does
    /// not defer anything new must not drop what an earlier
    /// `--ignore-scripts` install recorded.
    #[test]
    fn a_later_install_preserves_what_an_earlier_one_deferred() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        // Approved, so the second install's build phase reports nothing
        // deferred — the entry can only survive by being carried over.
        allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
        let deferred = read_pending_builds(&workspace);
        assert_eq!(deferred, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);

        let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        rerun.with_current_dir(&workspace).with_arg("install").assert().success();

        assert_eq!(read_pending_builds(&workspace), deferred);

        drop((root, mock_instance, rerun_root));
    }
}

/// Project (workspace/root) lifecycle scripts run during
/// `pacquet install` — preinstall, install, postinstall, preprepare,
/// prepare, postprepare — as opposed to the dependency build scripts
/// the [`dependency_build_scripts`] module above exercises.
mod project_scripts {
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
    use std::{fs, process::Command};

    /// A `node -e` lifecycle script that appends `<stage>\n` to
    /// `order.txt` in the script's cwd (the project root). pacquet
    /// passes Windows lifecycle scripts to `cmd /d /s /c` verbatim
    /// (matching Node's `windowsVerbatimArguments`), so the embedded
    /// quotes survive on every platform.
    fn append_order_script(stage: &str) -> String {
        format!(r#"node -e "require('fs').appendFileSync('order.txt','{stage}\n')""#)
    }

    fn project_with_lifecycle_scripts() -> serde_json::Value {
        serde_json::json!({
            "name": "project-with-lifecycle-scripts",
            "version": "1.0.0",
            "scripts": {
                "postpare": append_order_script("typo-never-runs"),
                "prepare": append_order_script("prepare"),
                "preprepare": append_order_script("preprepare"),
                "postprepare": append_order_script("postprepare"),
                "preinstall": append_order_script("preinstall"),
                "install": append_order_script("install"),
                "postinstall": append_order_script("postinstall"),
            },
        })
    }

    #[test]
    fn runs_project_lifecycle_scripts_in_order() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(
            stages,
            ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
        );

        drop((root, mock_instance));
    }

    #[test]
    fn runs_project_lifecycle_scripts_on_frozen_install() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();
        assert!(workspace.join("pnpm-lock.yaml").exists(), "first install should write a lockfile");
        fs::remove_file(workspace.join("order.txt")).expect("clear order.txt between installs");

        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_arg("install")
            .with_arg("--frozen-lockfile")
            .assert()
            .success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(
            stages,
            ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
        );

        drop((root, mock_instance));
    }

    #[test]
    fn failing_project_script_fails_the_install() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        // `exit 1` is shell-agnostic — a non-zero exit in both `sh -c`
        // and `cmd /d /s /c`. Mirrors pnpm's own test
        // (`preinstall: 'exit 1'`).
        let package_json = serde_json::json!({
            "name": "project-with-failing-script",
            "version": "1.0.0",
            "scripts": {
                "postinstall": "exit 1",
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().failure();

        drop((root, mock_instance));
    }

    /// The project's scripts run regardless of whether its `name`
    /// matches its directory.
    #[test]
    fn runs_scripts_when_project_name_differs_from_directory() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "name": "different-name",
            "version": "1.0.0",
            "scripts": {
                "preinstall": append_order_script("preinstall"),
                "install": append_order_script("install"),
                "postinstall": append_order_script("postinstall"),
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(stages, ["preinstall", "install", "postinstall"]);

        drop((root, mock_instance));
    }

    /// `INIT_CWD` is set to the lockfile directory for project scripts.
    #[test]
    fn project_script_sees_init_cwd() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "name": "project-reads-init-cwd",
            "version": "1.0.0",
            "scripts": {
                "postinstall":
                    r#"node -e "require('fs').writeFileSync('init-cwd.txt', process.env.INIT_CWD || '')""#,
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();

        let init_cwd =
            fs::read_to_string(workspace.join("init-cwd.txt")).expect("read init-cwd.txt");
        let canonical_workspace = fs::canonicalize(&workspace).expect("canonicalize workspace dir");
        let canonical_init_cwd =
            fs::canonicalize(init_cwd.trim()).expect("canonicalize INIT_CWD value");
        assert_eq!(canonical_init_cwd, canonical_workspace);

        drop((root, mock_instance));
    }

    /// An `updateConfig` pnpmfile hook that sets `extraEnv` exports those
    /// variables into the project's lifecycle-script environment. Mirrors
    /// the TypeScript CLI, where `config.extraEnv` is forwarded to
    /// lifecycle scripts.
    #[test]
    fn update_config_extra_env_reaches_lifecycle_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "name": "project-reads-extra-env",
            "version": "1.0.0",
            "scripts": {
                "postinstall":
                    r#"node -e "require('fs').writeFileSync('extra-env.txt', process.env.PNPM_BUILD_MARKER || '')""#,
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, PNPM_BUILD_MARKER: 'from-hook' }; return config } } }",
        )
        .expect("write pnpmfile");

        pacquet.with_arg("install").assert().success();

        let value =
            fs::read_to_string(workspace.join("extra-env.txt")).expect("read extra-env.txt");
        assert_eq!(value.trim(), "from-hook");

        drop((root, mock_instance));
    }

    /// A hook `extraEnv` that tries to override a reserved pnpm stamp
    /// (`INIT_CWD`) must NOT win: pnpm's own value is authoritative,
    /// matching TS `runLifecycleHook`. The script writes `INIT_CWD`,
    /// which must be the lockfile dir, not the hook's bogus value.
    #[test]
    fn update_config_extra_env_cannot_override_reserved_stamps() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "name": "project-reserved-stamp",
            "version": "1.0.0",
            "scripts": {
                "postinstall":
                    r#"node -e "require('fs').writeFileSync('init-cwd.txt', process.env.INIT_CWD || '')""#,
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, INIT_CWD: '/bogus-from-hook' }; return config } } }",
        )
        .expect("write pnpmfile");

        pacquet.with_arg("install").assert().success();

        let init_cwd =
            fs::read_to_string(workspace.join("init-cwd.txt")).expect("read init-cwd.txt");
        assert_ne!(init_cwd.trim(), "/bogus-from-hook", "hook extraEnv must not override INIT_CWD");
        let canonical_workspace = fs::canonicalize(&workspace).expect("canonicalize workspace dir");
        let canonical_init_cwd =
            fs::canonicalize(init_cwd.trim()).expect("canonicalize INIT_CWD value");
        assert_eq!(canonical_init_cwd, canonical_workspace);

        drop((root, mock_instance));
    }

    /// `pacquet add <pkg>` is a partial install, so the project's own
    /// lifecycle scripts must not run: `postinstall`/`prepare` are not
    /// executed after a named install.
    #[test]
    fn add_does_not_run_project_lifecycle_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let package_json = serde_json::json!({
            "name": "project-adding-a-dep",
            "version": "1.0.0",
            "scripts": {
                "preinstall": append_order_script("preinstall"),
                "install": append_order_script("install"),
                "postinstall": append_order_script("postinstall"),
                "prepare": append_order_script("prepare"),
            },
        });
        fs::write(workspace.join("package.json"), package_json.to_string())
            .expect("write package.json");

        pacquet.with_arg("add").with_arg("@pnpm.e2e/hello-world-js-bin-parent").assert().success();

        assert!(
            !workspace.join("order.txt").exists(),
            "named install (`add`) must not run the project's own lifecycle scripts",
        );

        drop((root, mock_instance));
    }

    /// `--ignore-scripts` suppresses the project's own lifecycle scripts:
    /// none of them run, so `order.txt` is never created. Mirrors pnpm,
    /// which skips the project lifecycle hooks alongside dependency build
    /// scripts under `ignoreScripts`.
    #[test]
    fn ignore_scripts_skips_project_lifecycle_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

        assert!(
            !workspace.join("order.txt").exists(),
            "no project lifecycle script should run under --ignore-scripts",
        );

        drop((root, mock_instance));
    }
}
