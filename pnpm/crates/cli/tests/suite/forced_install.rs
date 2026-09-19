use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn forced_install_removes_obsolete_child_links() {
    for frozen in [false, true] {
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
            serde_json::json!({ "dependencies": { "@pnpm.e2e/foobar": "100.0.0" } }).to_string(),
        )
        .unwrap();
        let config = workspace.join("pnpm-workspace.yaml");
        fs::write(
            &config,
            concat!(
                "packageExtensions:\n",
                "  '@pnpm.e2e/foobar':\n",
                "    dependencies:\n",
                "      is-positive: 1.0.0\n",
                "    optionalDependencies:\n",
                "      '@pnpm.e2e/removed-child': npm:is-positive@1.0.0\n",
            ),
        )
        .unwrap();
        pacquet
            .with_arg("install")
            .assert()
            .success();
        let modules = workspace.join("node_modules/.pnpm/@pnpm.e2e+foobar@100.0.0/node_modules");
        let obsolete = ["is-positive", "@pnpm.e2e/removed-child"].map(|alias| modules.join(alias));
        for child in &obsolete {
            assert!(child.symlink_metadata().is_ok(), "extension child must be linked: {child:?}");
        }
        fs::write(&config, "{}\n").unwrap();
        if frozen {
            pacquet_in(&workspace)
                .with_args(["install", "--lockfile-only"])
                .assert()
                .success();
        }
        let mode = if frozen { "--frozen-lockfile" } else { "--no-frozen-lockfile" };
        pacquet_in(&workspace)
            .with_args(["install", "--force", mode])
            .assert()
            .success();
        for child in &obsolete {
            assert_eq!(
                child
                    .symlink_metadata()
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::NotFound,
            );
        }
        assert!(
            modules.join("@pnpm.e2e/foobar/package.json").is_file(),
            "package must remain installed",
        );
        assert!(
            modules.join("@pnpm.e2e/foo/package.json").is_file(),
            "declared child must remain linked",
        );
        drop((root, mock_instance));
    }
}
