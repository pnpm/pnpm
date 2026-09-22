use pnpm_config::Config;
use pnpm_executor::ScriptEnvironment;
use std::{
    collections::HashMap,
    path::Path,
};

pub(crate) fn script_environment<'a>(
    config: &'a Config,
    init_cwd: &'a Path,
    extra_env: &'a HashMap<String, String>,
) -> ScriptEnvironment<'a> {
    ScriptEnvironment {
        init_cwd,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(&config.user_agent),
        extra_env,
    }
}
