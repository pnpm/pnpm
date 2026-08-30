//! Prepare a git-hosted package for installation.
//!
//! Decides whether a git-hosted package needs building, runs the
//! synthesized `<pm>-install` (which transitively runs npm/yarn/pnpm's
//! built-in `prepare` lifecycle), then any remaining `prepublish*`
//! scripts. Honors the `allowBuild` gate, and rejects sub-paths that
//! escape the git root via [`safe_join_path`].

use crate::{
    error::PreparePackageError,
    pm_shims::{shim_names, write_pm_shims},
    preferred_pm::{PreferredPm, WantedPm, detect_wanted_pm},
};
use pnpm_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook,
};
use pnpm_network::redact_and_sanitize;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex, PoisonError},
};

/// Scripts to re-run after `<pm>-install` finishes. `prepare` itself
/// runs automatically as part of `<pm>-install` (npm/yarn/pnpm fold it
/// into the install lifecycle), so we don't need to invoke it
/// separately.
///
/// Note: `prepublishOnly` is intentionally omitted here — neither npm
/// nor Yarn run it for git-hosted deps.
const PREPUBLISH_SCRIPTS: &[&str] = &["prepublish", "prepack", "publish"];

/// Closure shape used to ask the install policy whether the package at
/// a dep path is allowed to run lifecycle scripts.
///
/// We pass a closure rather than `&AllowBuildPolicy` so the
/// `pnpm-git-fetcher` crate stays free of a back-edge into
/// `pnpm-package-manager`. The caller adapts whatever policy
/// structure it has into this shape.
pub type AllowBuildFn<'a> = Box<dyn Fn(&str) -> bool + Send + Sync + 'a>;
pub type AllowBuildRef<'a> = &'a (dyn Fn(&str) -> bool + Send + Sync);

/// Caller-supplied context for [`prepare_package`].
pub struct PreparePackageOptions<'a> {
    pub allow_build: AllowBuildFn<'a>,
    /// The package's resolution id — the bare `git+…#<commit>` or
    /// archive URL. The gated dep path is synthesized from it and the
    /// fetched manifest's name, so the policy sees the same
    /// `<name>@<id>` key a lockfile would record.
    pub pkg_resolution_id: &'a str,
    pub ignore_scripts: bool,
    pub unsafe_perm: bool,
    pub user_agent: Option<&'a str>,
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    pub script_shell: Option<&'a Path>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    /// The running pnpm, which the package-manager shims forward to.
    /// Without it pnpm cannot provide the package manager a dependency
    /// asks for, and the build falls back to whatever the host has.
    pub pnpm_execpath: Option<&'a Path>,
    pub extra_bin_paths: &'a [PathBuf],
    pub extra_env: &'a HashMap<String, String>,
}

/// Result of [`prepare_package`]. `should_be_built` drives the
/// `built` dimension of the git-hosted store-index key.
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

    assert_package_build_allowed(opts.allow_build.as_ref(), opts.pkg_resolution_id, &manifest)?;

    let name = manifest.get("name").and_then(Value::as_str).unwrap_or("");
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or("");
    let wanted_pm = detect_wanted_pm(git_root_dir, Some(&manifest));
    let pm = wanted_pm.pm;
    let dep_path = format!("{name}@{version}");

    let mut extra_bin_paths = opts.extra_bin_paths.to_vec();
    // Kept alive until the prepare is over: dropping it takes the shims
    // with it.
    let shims_dir = provide_wanted_pm::<Reporter>(&wanted_pm, &dep_path, opts.pnpm_execpath)?;
    if let Some(dir) = shims_dir.as_ref() {
        extra_bin_paths.insert(0, dir.path().to_path_buf());
    }

    let run_opts = RunPostinstallHooks {
        dep_path: &dep_path,
        pkg_root: &pkg_dir,
        root_modules_dir: &pkg_dir,
        init_cwd: &pkg_dir,
        extra_bin_paths: &extra_bin_paths,
        extra_env: opts.extra_env,
        node_execpath: opts.node_execpath,
        npm_execpath: opts.npm_execpath,
        node_gyp_path: None,
        user_agent: opts.user_agent,
        unsafe_perm: opts.unsafe_perm,
        node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
        scripts_prepend_node_path: opts.scripts_prepend_node_path,
        script_shell: opts.script_shell,
        shell_emulator: false,
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

    // Remove the install-time `node_modules` so the deps don't leak
    // into the CAS. Ignore `NotFound` (the script may not have
    // populated `node_modules` at all).
    let node_modules = pkg_dir.join("node_modules");
    if let Err(error) = fs::remove_dir_all(&node_modules)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(PreparePackageError::Io(error));
    }

    Ok(PreparedPackage { pkg_dir, should_be_built: true })
}

/// Whether the package manager on `PATH` can install what the dependency
/// ships.
///
/// Having the command is not enough when the lockfile constrains the
/// line: Yarn Classic cannot read a Berry lockfile, and a host copy from
/// the wrong line is no more usable than no copy at all. A host version
/// that cannot be read counts as unusable, so the dependency gets the one
/// it asked for rather than a coin flip.
/// Put the package manager the dependency asks for on the build's `PATH`,
/// returning the scratch directory its shims live in — the caller keeps
/// that alive for the prepare, because dropping it takes the shims with
/// it.
///
/// pnpm provides the package manager when the dependency pinned a version
/// — that pin is what its authors test against — or when the host cannot
/// satisfy what the dependency ships. Otherwise the host's own install is
/// left to do the job it has always done, which is also what happens when
/// the shims cannot be written for an unpinned one.
fn provide_wanted_pm<Reporter: self::Reporter>(
    wanted_pm: &WantedPm,
    dep_path: &str,
    pnpm_execpath: Option<&Path>,
) -> Result<Option<tempfile::TempDir>, PreparePackageError> {
    if !wanted_pm.pinned && host_can_prepare(wanted_pm) {
        return Ok(None);
    }
    let Some(pnpm_execpath) = pnpm_execpath else {
        if wanted_pm.pinned {
            // Without the running pnpm there is nothing to forward a shim
            // to, which is the case where pnpm is embedded rather than run
            // as a command. The host's package manager prepares the
            // package instead, so the dependency is built by a version it
            // did not ask for and the user hears about it.
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Cannot provide {} to prepare {dep_path}: preparing it with the host's instead.",
                    describe_wanted_pm(wanted_pm),
                ),
                prefix: String::new(),
            }));
        }
        return Ok(None);
    };
    match provide_package_manager(wanted_pm, pnpm_execpath) {
        Ok(dir) => Ok(Some(dir)),
        // A pin is what the dependency's own authors test against, so
        // preparing it with whatever the host happens to have would
        // silently produce a different tree.
        Err(error) if wanted_pm.pinned => Err(PreparePackageError::PackageManagerUnavailable {
            package_manager: describe_wanted_pm(wanted_pm),
            source: error,
        }),
        Err(error) => {
            let name = wanted_pm.pm.name();
            tracing::warn!(
                target: "pacquet::git_fetcher",
                "could not provide {name} for the build: {error}",
            );
            Ok(None)
        }
    }
}

/// Write the shims for `wanted` into a scratch directory, returning it so
/// the caller can put it on the build's `PATH` and drop it afterwards.
fn provide_package_manager(
    wanted: &WantedPm,
    pnpm_execpath: &Path,
) -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    write_pm_shims(dir.path(), wanted, pnpm_execpath)?;
    Ok(dir)
}

fn describe_wanted_pm(wanted: &WantedPm) -> String {
    match &wanted.version_spec {
        Some(version_spec) => format!("{}@{version_spec}", wanted.pm.name()),
        None => wanted.pm.name().to_string(),
    }
}

fn host_can_prepare(wanted: &WantedPm) -> bool {
    // Every unpinned git dependency asks this, and the answer cannot
    // change under a running install: the host's package managers are not
    // pnpm's to install.
    /// A package manager and the version the dependency wants of it.
    type HostQuestion = (PreferredPm, Option<String>);

    static ANSWERS: LazyLock<Mutex<HashMap<HostQuestion, bool>>> = LazyLock::new(Mutex::default);

    // A panic while the cache was held says nothing about the answers
    // already in it, and refusing to prepare a dependency over it would
    // be a worse outcome than a stale entry could ever be.
    let lock = || ANSWERS.lock().unwrap_or_else(PoisonError::into_inner);

    let key = (wanted.pm, wanted.version_spec.clone());
    if let Some(answer) = lock().get(&key) {
        return *answer;
    }
    let answer = probe_host(wanted);
    lock().insert(key, answer);
    answer
}

fn probe_host(wanted: &WantedPm) -> bool {
    let wanted_range =
        wanted.version_spec.as_deref().and_then(|range| node_semver::Range::parse(range).ok());
    // A dependency's scripts reach for any of the package manager's names
    // — `yarnpkg` as readily as `yarn` — and nothing says two of them on
    // one host are the same install, so each has to answer for itself.
    shim_names(wanted.pm).all(|name| {
        let Ok(program) = which::which(name) else {
            return false;
        };
        let Some(wanted_range) = wanted_range.as_ref() else {
            return true;
        };
        let Ok(output) = Command::new(program).arg("--version").output() else {
            return false;
        };
        // A version printed by a command that then failed says nothing
        // about what that command can do.
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::trim)
                .and_then(|version| node_semver::Version::parse(version).ok())
                .is_some_and(|version| version.satisfies(wanted_range))
    })
}

pub fn assert_package_build_allowed(
    allow_build: AllowBuildRef<'_>,
    pkg_resolution_id: &str,
    manifest: &Value,
) -> Result<(), PreparePackageError> {
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or("");
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or("");
    let allow_build_dep_path = format!("{name}@{pkg_resolution_id}");
    if allow_build(&allow_build_dep_path) {
        return Ok(());
    }
    Err(PreparePackageError::NotAllowed {
        name: name.to_string(),
        version: version.to_string(),
        dep_path: redact_and_sanitize(&allow_build_dep_path),
    })
}

/// Decide whether the package needs building.
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
///
/// `sub` is a resolution's `path` field, which keeps the leading slash
/// of the `#path:/packages/foo` specifier it came from. That slash is
/// rooted at the repo, not the filesystem, so it is stripped before
/// joining — [`Path::join`] would otherwise discard `root` and treat
/// the whole thing as absolute, unlike the `path.join` upstream uses.
pub(crate) fn safe_join_path(
    root: &Path,
    sub: Option<&str>,
) -> Result<PathBuf, PreparePackageError> {
    let sub = sub.unwrap_or("").trim_start_matches(['/', '\\']);
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
/// so the next `run_lifecycle_hook` invocation can look it up.
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
