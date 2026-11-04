use super::{ProjectScripts, env, script_concurrency};

pub(super) struct GithubGroup;

pub(super) fn start(run: &ProjectScripts<'_, '_, '_>, selected: &str) -> Option<GithubGroup> {
    let options = run.options;
    if options.output.silent
        || options.args.workspace.parallel
        || script_concurrency(
            options.config,
            options.node.scripts.len(),
            options.args.workspace.parallel,
            options.args.sequential,
        ) > 1
        || env::var_os("GITHUB_ACTIONS").is_none_or(|value| value != "true")
        || env::var_os("CI").is_some_and(|value| value == "false")
    {
        return None;
    }
    let manifest = run.manifest.value();
    let name = manifest["name"].as_str().unwrap_or("unknown");
    let version = manifest["version"]
        .as_str()
        .filter(|version| !version.is_empty())
        .map(|version| format!("@{version}"))
        .unwrap_or_default();
    let root = options.config.workspace_dir.as_deref().unwrap_or(options.init_cwd);
    let prefix = pathdiff::diff_paths(&options.node.project, root)
        .unwrap_or_else(|| options.node.project.clone());
    let title = format!("{name}{version} : {selected} {}", prefix.display())
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    pnpm_diagnostics::println!("::group::{title}");
    Some(GithubGroup)
}

impl Drop for GithubGroup {
    fn drop(&mut self) {
        pnpm_diagnostics::println!("::endgroup::");
    }
}
