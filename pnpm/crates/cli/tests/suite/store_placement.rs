//! Commands that never open the package store leave the project directory
//! alone. Placing the default store writes a hardlink probe into the
//! project directory, and file watchers such as the Nx daemon, which runs
//! `pnpm view` on start, react to that write.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

#[test]
fn commands_without_store_access_do_not_modify_the_project_directory() {
    let mut registry = mockito::Server::new();
    let _not_found = registry
        .mock("GET", mockito::Matcher::Any)
        .with_status(404)
        .create();
    let registry_arg = format!("--registry={}/", registry.url());
    let cases: [(&[&str], bool); 13] = [
        (&["root"], true),
        (&["prefix"], true),
        (&["bin"], true),
        (&["config", "get", "registry"], true),
        (&["run", "noop"], true),
        (&["exec", "node", "-e", "0"], true),
        (&["view", "nx", "version", &registry_arg], false),
        (&["ping", &registry_arg], false),
        (&["search", "nx", &registry_arg], false),
        (&["dist-tag", "ls", "nx", &registry_arg], false),
        (&["owner", "ls", "nx", &registry_arg], false),
        (&["whoami", &registry_arg], false),
        (&["docs", "nx", &registry_arg], false),
    ];
    for (args, succeeds) in cases {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(
            workspace.join("package.json"),
            r#"{"name":"project","version":"1.0.0","scripts":{"noop":"node -e 0"}}"#,
        )
        .expect("write package.json");
        let modified = freeze_mtime(&workspace);

        let output = isolated(pacquet, root.path())
            .with_args(args)
            .output()
            .expect("run pnpm");
        assert_eq!(output.status.success(), succeeds, "{args:?}: {output:?}");
        assert!(!root.path().join("pnpm-home").exists(), "{args:?} created the pnpm home");
        if let Some(modified) = modified {
            // A probe that is created and deleted leaves no file, only a new
            // directory timestamp.
            assert_eq!(
                fs::metadata(&workspace)
                    .unwrap()
                    .modified()
                    .unwrap(),
                modified,
                "{args:?} modified the project directory",
            );
        }
        let entries: Vec<_> = fs::read_dir(&workspace)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, ["package.json"], "{args:?}");
    }
}

/// Pin the directory's modification time to a fixed past instant and return
/// it, on the platforms whose directory timestamps track entry changes.
#[cfg(unix)]
fn freeze_mtime(dir: &Path) -> Option<std::time::SystemTime> {
    use std::time::{Duration, UNIX_EPOCH};
    let modified = UNIX_EPOCH + Duration::from_hours(262_968);
    fs::File::open(dir)
        .expect("open project directory")
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("set directory timestamp");
    Some(modified)
}

#[cfg(not(unix))]
fn freeze_mtime(_dir: &Path) -> Option<std::time::SystemTime> {
    None
}

fn isolated(mut command: Command, root: &Path) -> Command {
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    command.env_remove("COREPACK_ROOT");
    // The project is never installed, so `run` and `exec` would otherwise
    // spawn an install, which does open the store.
    command.env("pnpm_config_verify_deps_before_run", "false");
    command
}
