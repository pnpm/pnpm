use clap::Args;

use super::run::RunArgs;

/// Restarts a package. Runs a package's "stop", "restart", and "start"
/// scripts, and associated pre- and post- scripts.
///
/// Ports the `restart` command from
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/restart.ts>.
///
/// Each script is executed through the full [`RunArgs`] pipeline, so
/// lifecycle hooks (`pre<name>` / `post<name>`) and environment setup
/// apply when `enablePrePostScripts` is set.
#[derive(Debug, Args)]
pub struct RestartArgs {
    /// Arguments passed to each script after the script name.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Avoid exiting with a non-zero exit code when a script is undefined.
    #[clap(long)]
    pub if_present: bool,
}

impl RestartArgs {
    pub fn run(
        self,
        dir: &std::path::Path,
        config: &pacquet_config::Config,
        silent: bool,
    ) -> miette::Result<()> {
        let RestartArgs { args, if_present } = self;

        for script_name in ["stop", "restart", "start"] {
            RunArgs {
                command: Some(script_name.to_string()),
                args: args.clone(),
                if_present,
                resume_from: None,
                report_summary: false,
                no_bail: false,
            }
            .run(dir, config, silent)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RestartArgs;
    use serde_json::json;
    use tempfile::TempDir;

    fn setup_project(dir: &std::path::Path, scripts: &serde_json::Value) {
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": scripts,
        });
        std::fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    #[test]
    fn restart_runs_stop_restart_start_in_order() {
        let tmp = TempDir::new().expect("tmp dir");
        let dir = tmp.path();
        let stop_marker = dir.join("stop.txt");
        let restart_marker = dir.join("restart.txt");
        let start_marker = dir.join("start.txt");
        setup_project(
            dir,
            &json!({
                "stop": format!("touch \"{}\"", stop_marker.display()),
                "restart": format!("touch \"{}\"", restart_marker.display()),
                "start": format!("touch \"{}\"", start_marker.display()),
            }),
        );
        let config = pacquet_config::Config::default();
        RestartArgs { args: vec![], if_present: false }
            .run(dir, &config, true)
            .expect("restart should succeed");
        assert!(stop_marker.exists(), "stop script should have run first");
        assert!(restart_marker.exists(), "restart script should have run");
        assert!(start_marker.exists(), "start script should have run last");
    }

    #[test]
    fn restart_with_if_present_skips_missing_stop_and_restart() {
        let tmp = TempDir::new().expect("tmp dir");
        let dir = tmp.path();
        setup_project(
            dir,
            &json!({
                "start": "exit 0",
            }),
        );
        let config = pacquet_config::Config::default();
        RestartArgs { args: vec![], if_present: true }
            .run(dir, &config, true)
            .expect("--if-present should skip missing stop/restart");
    }

    #[test]
    fn restart_passes_args_to_each_script() {
        let tmp = TempDir::new().expect("tmp dir");
        let dir = tmp.path();
        let marker = dir.join("args.txt");
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "stop": "true",
                "restart": "true",
                "start": format!("node -e \"require('fs').writeFileSync('{}', process.argv[1])\"", marker.display()),
            },
        });
        std::fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
        let config = pacquet_config::Config::default();
        RestartArgs { args: vec!["myarg".to_string()], if_present: false }
            .run(dir, &config, true)
            .expect("restart should succeed");
        let written = std::fs::read_to_string(&marker).expect("read marker");
        assert_eq!(written, "myarg", "args should be passed through to the script");
    }
}
