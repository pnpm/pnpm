//! Port of pnpm's
//! [`exec/prepare-package`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts).
//!
//! Decides whether a git-hosted package needs building, runs the
//! synthesized `<pm>-install` (which transitively runs npm/yarn/pnpm's
//! built-in `prepare` lifecycle), then any remaining `prepublish*`
//! scripts. Honors the same `allowBuild` gate pnpm uses, and rejects
//! sub-paths that escape the git root via `safe_join_path` (mirrors
//! upstream's `safeJoinPath` at
//! [`index.ts:92-103`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L92-L103)).

use crate::{error::PreparePackageError, preferred_pm::detect_preferred_pm};
use pacquet_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook,
};
use pacquet_package_manifest::safe_read_package_json_from_dir;
use pacquet_reporter::Reporter;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// Scripts to re-run after `<pm>-install` finishes. `prepare` itself
/// runs automatically as part of `<pm>-install` (npm/yarn/pnpm fold it
/// into the install lifecycle), so we don't need to invoke it
/// separately. Matches upstream's set at
/// [`exec/prepare-package/src/index.ts:14-20`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L14-L20).
///
/// Note: pnpm intentionally omits `prepublishOnly` here — neither npm
/// nor Yarn run it for git-hosted deps.
const PREPUBLISH_SCRIPTS: &[&str] = &["prepublish", "prepack", "publish"];

/// Closure shape used to ask the install policy whether the package at
/// a dep path is allowed to run lifecycle scripts.
///
/// We pass a closure rather than `&AllowBuildPolicy` so the
/// `pacquet-git-fetcher` crate stays free of a back-edge into
/// `pacquet-package-manager`. The caller adapts whatever policy
/// structure it has into this shape.
pub type AllowBuildFn<'a> = Box<dyn Fn(&str) -> bool + Send + Sync + 'a>;
pub type AllowBuildRef<'a> = &'a (dyn Fn(&str) -> bool + Send + Sync);

/// Caller-supplied context for [`prepare_package`].
pub struct PreparePackageOptions<'a> {
    pub allow_build: AllowBuildFn<'a>,
    pub dep_path: &'a str,
    pub ignore_scripts: bool,
    pub unsafe_perm: bool,
    pub user_agent: Option<&'a str>,
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    pub script_shell: Option<&'a Path>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    pub extra_bin_paths: &'a [PathBuf],
    pub extra_env: &'a HashMap<String, String>,
}

/// Result of [`prepare_package`]. `should_be_built` lines up with
/// upstream's `shouldBeBuilt` — drives the `built` dimension of the
/// git-hosted store-index key.
#[derive(Debug)]
pub struct PreparedPackage {
    pub pkg_dir: PathBuf,
    pub should_be_built: bool,
}

/// Read the manifest, decide whether the package needs building, and
/// run the appropriate lifecycle scripts. Returns `should_be_built:
/// false` early when there's nothing to do; otherwise runs
/// `<pm>-install` plus any defined `prepublish` / `prepack` / `publish`
/// hooks, then deletes `node_modules` so the install-time deps don't
/// leak into the CAS.
///
/// Mirrors upstream's `preparePackage` at
/// [`index.ts:29-80`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L29-L80).
pub fn prepare_package<Reporter: self::Reporter>(
    opts: &PreparePackageOptions<'_>,
    git_root_dir: &Path,
    sub_dir: Option<&str>,
) -> Result<PreparedPackage, PreparePackageError> {
    let pkg_dir = safe_join_path(git_root_dir, sub_dir)?;
    let manifest =
        safe_read_package_json_from_dir(&pkg_dir).map_err(PreparePackageError::ReadManifest)?;

    let Some(manifest) = manifest else {
        return Ok(PreparedPackage { pkg_dir, should_be_built: false });
    };
    let scripts = manifest.get("scripts").and_then(Value::as_object);
    if scripts.is_none_or(serde_json::Map::is_empty)
        || !package_should_be_built(&manifest, &pkg_dir)
    {
        return Ok(PreparedPackage { pkg_dir, should_be_built: false });
    }
    if opts.ignore_scripts {
        return Ok(PreparedPackage { pkg_dir, should_be_built: true });
    }

    // `allowBuild` check before any spawn. Upstream throws when
    // `opts.allowBuild?.(depPath)` is missing or false, with
    // GIT_DEP_PREPARE_NOT_ALLOWED. The manifest comes from the fetched
    // artifact itself, so its name and version only feed the error
    // message; the dep path is the gated identity.
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or("");
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or("");
    if !(opts.allow_build)(opts.dep_path) {
        return Err(PreparePackageError::NotAllowed {
            name: name.to_string(),
            version: version.to_string(),
        });
    }

    let pm = detect_preferred_pm(git_root_dir);
    let dep_path = format!("{name}@{version}");

    let run_opts = RunPostinstallHooks {
        dep_path: &dep_path,
        pkg_root: &pkg_dir,
        root_modules_dir: &pkg_dir,
        init_cwd: &pkg_dir,
        extra_bin_paths: opts.extra_bin_paths,
        extra_env: opts.extra_env,
        node_execpath: opts.node_execpath,
        npm_execpath: opts.npm_execpath,
        node_gyp_path: None,
        user_agent: opts.user_agent,
        unsafe_perm: opts.unsafe_perm,
        node_gyp_bin: None,
        scripts_prepend_node_path: opts.scripts_prepend_node_path,
        script_shell: opts.script_shell,
        optional: false,
    };

    let parent_env: HashMap<String, String> = std::env::vars().collect();
    let mut working_manifest = manifest.clone();
    let install_stage = format!("{}-install", pm.name());
    let install_script = format!("{} install", pm.name());
    inject_script(&mut working_manifest, &install_stage, &install_script);
    run_lifecycle_hook::<Reporter>(
        &install_stage,
        &install_script,
        &run_opts,
        &working_manifest,
        &parent_env,
    )
    .map_err(map_lifecycle_err)?;

    for &script_name in PREPUBLISH_SCRIPTS {
        let Some(script_body) = working_manifest
            .get("scripts")
            .and_then(|s| s.get(script_name))
            .and_then(Value::as_str)
            .filter(|script| !script.is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        let (stage, script) = if pm.name() == "pnpm" {
            (script_name.to_string(), script_body)
        } else {
            let synthesized_stage = format!("{}-run-{}", pm.name(), script_name);
            let synthesized = format!("{} run {}", pm.name(), script_name);
            inject_script(&mut working_manifest, &synthesized_stage, &synthesized);
            (synthesized_stage, synthesized)
        };
        run_lifecycle_hook::<Reporter>(&stage, &script, &run_opts, &working_manifest, &parent_env)
            .map_err(map_lifecycle_err)?;
    }

    // Upstream `rimraf`s the install-time `node_modules` so the deps
    // don't leak into the CAS. Ignore `NotFound` (the script may not
    // have populated `node_modules` at all).
    let node_modules = pkg_dir.join("node_modules");
    if let Err(error) = fs::remove_dir_all(&node_modules)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(PreparePackageError::Io(error));
    }

    Ok(PreparedPackage { pkg_dir, should_be_built: true })
}

/// Decide whether the package needs building. Mirrors upstream's
/// [`packageShouldBeBuilt`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L82-L90).
fn package_should_be_built(manifest: &Value, pkg_dir: &Path) -> bool {
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return false;
    };
    if scripts.get("prepare").and_then(Value::as_str).is_some_and(|script| !script.is_empty()) {
        return true;
    }
    let has_prepublish_script = PREPUBLISH_SCRIPTS.iter().any(|name| {
        scripts.get(*name).and_then(Value::as_str).is_some_and(|script| !script.is_empty())
    });
    if !has_prepublish_script {
        return false;
    }
    let main_file = manifest.get("main").and_then(Value::as_str).unwrap_or("index.js");
    !pkg_dir.join(main_file).exists()
}

/// Join `sub` onto `root` and reject results that climb outside.
/// Mirrors upstream's [`safeJoinPath`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L92-L103).
fn safe_join_path(root: &Path, sub: Option<&str>) -> Result<PathBuf, PreparePackageError> {
    let sub = sub.unwrap_or("");
    let joined = if sub.is_empty() { root.to_path_buf() } else { root.join(sub) };
    let canonical_root = root.canonicalize().map_err(PreparePackageError::Io)?;
    let Ok(canonical_joined) = joined.canonicalize() else {
        return Err(PreparePackageError::InvalidPath { path: sub.to_string() });
    };
    if !canonical_joined.starts_with(&canonical_root) {
        return Err(PreparePackageError::InvalidPath { path: sub.to_string() });
    }
    if !canonical_joined.is_dir() {
        return Err(PreparePackageError::InvalidPath { path: sub.to_string() });
    }
    Ok(joined)
}

/// Write `(stage, script)` into the working manifest's `scripts` map
/// so the next `run_lifecycle_hook` invocation can look it up. Matches
/// upstream's `manifest.scripts[installScriptName] = ...` mutation at
/// [`index.ts:57`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L57).
fn inject_script(manifest: &mut Value, stage: &str, script: &str) {
    let scripts = manifest.get_mut("scripts").and_then(Value::as_object_mut);
    if let Some(scripts) = scripts {
        scripts.insert(stage.to_string(), Value::String(script.to_string()));
    }
}

fn map_lifecycle_err(source: LifecycleScriptError) -> PreparePackageError {
    PreparePackageError::LifecycleFailed { source }
}

#[cfg(test)]
mod tests;
