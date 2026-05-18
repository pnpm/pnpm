use crate::registry_mock;
use std::{path::PathBuf, process::Command, sync::OnceLock};

static LAUNCH_SCRIPT: OnceLock<PathBuf> = OnceLock::new();

/// Path to the `launch.mjs` wrapper that drives `@pnpm/registry-mock`
/// via its programmatic API. The wrapper is required because the
/// package's default CLI export does not thread `useNodeVersion`
/// through — and verdaccio 5.33 (the version v6 of `@pnpm/registry-mock`
/// bundles) rejects its 64-character storage secret on Node 22+, so
/// running it under the host Node fails. Pacquet pins
/// `useNodeVersion: '20.16.0'` in the wrapper to match pnpm's jest
/// `globalSetup` shape.
fn launch_script() -> &'static PathBuf {
    LAUNCH_SCRIPT.get_or_init(|| registry_mock().join("launch.mjs"))
}

/// Returns a [`Command`] pre-populated with `node <launch.mjs>`. The
/// caller appends `prepare` (to publish fixtures) or omits the arg
/// (to launch the server) and any environment / stdio setup.
pub fn node_registry_mock() -> Command {
    let mut cmd = Command::new("node");
    cmd.arg(launch_script());
    cmd
}
