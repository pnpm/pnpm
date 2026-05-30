mod known_failures {
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pacquet_testing_utils::{
        allow_known_failure,
        bin::{AddMockedRegistry, CommandTempCwd},
        known_failure::{KnownFailure, KnownResult},
    };
    use pipe_trait::Pipe;
    use std::{fs, path::Path};

    fn build_deps_ran(_workspace: &Path) -> KnownResult<()> {
        Err(KnownFailure::new(
            "lifecycle scripts only run in the frozen-lockfile path; \
             the non-frozen path does not write a lockfile so these \
             tests cannot exercise --frozen-lockfile yet. \
             Additionally, bin linking (#330) is not implemented.",
        ))
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L26>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L121>
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
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                },
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

        let pkg_dir = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0\
             /node_modules/@pnpm.e2e/install-script-example",
        );

        eprintln!("Checking generated-by-install.js exists...");
        assert!(pkg_dir.join("generated-by-install.js").exists());

        drop((root, mock_instance));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L303>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        let ts_b = output_b[0].as_u64().expect("B timestamp");
        let ts_a = output_a[0].as_u64().expect("A timestamp");
        eprintln!("Checking B ran before A (B={ts_b}, A={ts_a})...");
        assert!(ts_b < ts_a, "dependency B should run before dependent A");

        drop((root, mock_instance));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L331>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        allow_known_failure!(build_deps_ran(&workspace));

        eprintln!("Checking generated bins are executable after frozen reinstall...");
        #[cfg(unix)]
        {
            use pacquet_testing_utils::fs::is_path_executable;
            assert!(is_path_executable(&node_modules.join(".bin/cmd1")));
            assert!(is_path_executable(&node_modules.join(".bin/cmd2")));
        }

        drop((root, mock_instance, frozen_root));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L372>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        drop((root, mock_instance));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L445>
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
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                },
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        drop((root, mock_instance));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L466>
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
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                },
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        // TODO: assert the `pnpm:ignored-scripts` reporter event lists
        // `@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0` here. Pacquet
        // does not emit that channel yet (see issue <https://github.com/pnpm/pacquet/issues/397>).

        eprintln!(
            "Re-running install with explicit denial of pre-and-postinstall-scripts-example...",
        );
        fs::remove_dir_all(&node_modules).expect("remove node_modules");
        let updated_package_json = serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0",
            },
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                    "@pnpm.e2e/pre-and-postinstall-scripts-example": false,
                },
            },
        });
        fs::write(&manifest_path, updated_package_json.to_string()).expect("update package.json");

        let CommandTempCwd { pacquet: frozen_pacquet, root: frozen_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        frozen_pacquet
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        assert!(!denied_pkg.join("generated-by-preinstall.js").exists());
        assert!(!denied_pkg.join("generated-by-postinstall.js").exists());
        assert!(allowed_pkg.join("generated-by-install.js").exists());

        // TODO: assert the `pnpm:ignored-scripts` reporter event lists no
        // package names this time — explicit denial moves the package from
        // "ignored" to "silently skipped".

        drop((root, mock_instance, frozen_root));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L543>
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
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example@1.0.0": true,
                },
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

        drop((root, mock_instance));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L552>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-installer/test/install/lifecycleScripts.ts#L724>
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
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                },
            },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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
        let updated_manifest = serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0",
            },
            "pnpm": {
                "allowBuilds": {
                    "@pnpm.e2e/install-script-example": true,
                    "@pnpm.e2e/pre-and-postinstall-scripts-example": true,
                },
            },
        });
        fs::write(&manifest_path, updated_manifest.to_string()).expect("write package.json");

        let CommandTempCwd { pacquet: frozen_pacquet, root: frozen_root, .. } =
            CommandTempCwd::init().add_mocked_registry();
        frozen_pacquet
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        allow_known_failure!(build_deps_ran(&workspace));

        eprintln!("Checking all scripts ran after allowBuilds change...");
        assert!(install_pkg.join("generated-by-install.js").exists());
        assert!(scripts_pkg.join("generated-by-preinstall.js").exists());
        assert!(scripts_pkg.join("generated-by-postinstall.js").exists());

        drop((root, mock_instance, frozen_root));
    }

    // Ported from <https://github.com/pnpm/pnpm/blob/7e91e4b35f/installing/deps-restorer/test/index.ts#L362>
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

        eprintln!("Running pacquet install...");
        pacquet.with_arg("install").assert().success();

        allow_known_failure!(build_deps_ran(&workspace));

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
}

/// Project (workspace/root) lifecycle scripts run during
/// `pacquet install` — preinstall, install, postinstall, preprepare,
/// prepare, postprepare — as opposed to the dependency build scripts
/// the `known_failures` module above exercises.
///
/// Ported from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/test/install/lifecycleScripts.ts>.
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

        // First install resolves and writes pnpm-lock.yaml (and runs
        // the scripts once).
        pacquet.with_arg("install").assert().success();
        assert!(workspace.join("pnpm-lock.yaml").exists(), "first install should write a lockfile");
        fs::remove_file(workspace.join("order.txt")).expect("clear order.txt between installs");

        // Frozen reinstall must re-run the project scripts.
        Command::cargo_bin("pacquet")
            .expect("find the pacquet binary")
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

    /// Ported from
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/test/install/lifecycleScripts.ts#L155>
    /// — the project's scripts run regardless of whether its `name`
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

    /// Ported from
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/test/install/lifecycleScripts.ts#L187>
    /// — `INIT_CWD` is set to the lockfile directory for project scripts.
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

    /// `pacquet add <pkg>` is a partial install (pnpm's
    /// `mutation: 'installSome'`), so the project's own lifecycle
    /// scripts must not run. Ported from
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/pnpm/test/install/lifecycleScripts.ts#L66-L90>
    /// (`postinstall`/`prepare` are not executed after a named install).
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
}
