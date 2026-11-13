use crate::{port_to_url::port_to_url, seed_storage::seed_runtime_storage, workspace_root};
use std::{
    env, fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

// The tests stand in for `pnpr` with a shell script.
#[cfg(all(test, unix))]
mod tests;

/// Locate the `pnpr` binary built into the cargo workspace's
/// `target/<profile>/` dir.
///
/// `assert_cmd::cargo::cargo_bin` (the obvious choice) panics here:
/// it relies on `CARGO_BIN_EXE_<name>`, which cargo only injects
/// when running an *integration test* of the crate that defines the
/// binary. Both `pnpm-registry-mock launch` (the recipe invoked
/// from `just registry-mock launch`) and the auto-spawn path in
/// pacquet's install tests run from a different crate, so the env
/// var is unset and `cargo_bin` aborts. We resolve the path
/// ourselves:
///
/// 1. Honor `CARGO_BIN_EXE_pnpr` if set — this is the case
///    inside pnpr's own integration tests.
/// 2. Prefer the release binary if one exists at
///    `$CARGO_TARGET_DIR/release/pnpr`. Critical for the
///    integrated benchmark: comparing a debug-Rust mock to a
///    JIT-optimized verdaccio is apples to oranges — the install
///    measures 20%+ slower purely from unoptimized JSON parse +
///    serialize on every request. Tests don't need the release
///    binary, but having a release build override the debug one is
///    always the right choice when both exist.
/// 3. Fall back to `$CARGO_TARGET_DIR/debug/pnpr` for
///    local dev where only `cargo build` ran.
fn pnpr_binary() -> PathBuf {
    if let Some(path) = env::var_os("CARGO_BIN_EXE_pnpr") {
        return PathBuf::from(path);
    }
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from);
    let exe = format!("pnpr{}", env::consts::EXE_SUFFIX);
    let release = target_dir.join("release").join(&exe);
    if release.is_file() {
        return release;
    }
    target_dir.join("debug").join(&exe)
}

/// Build a [`Command`] that spawns the `pnpr` binary in
/// proxy mode against the storage built from the in-repo fixtures,
/// with `registry.npmjs.org` as the upstream.
///
/// The storage holds the fixture packages (`@foo`, `@pnpm.e2e`, ...)
/// and anything else (e.g. extra npm packages the benchmark lockfile
/// pulls) is fetched from npmjs.org on demand.
///
/// `--packument-ttl 31536000` (one year) keeps the fixture
/// packuments authoritative across a test run: their on-disk
/// mtime is whenever the npm tarball was built, which is far older
/// than any sane TTL, so a short TTL would mark them stale and try
/// to refetch from npm — where the fixtures don't exist — and 404.
///
/// `pnpr` is a workspace crate; run
/// `cargo build -p pnpr` once before invoking the mock if
/// it isn't already built. `--public-url` defaults to
/// `http://localhost:<port>` so the tarball URLs the registry rewrites
/// sit under the URL [`crate::RegistryInfo::url`] returns. The
/// integrated benchmark can override it when a proxy fronts the registry
/// port: packuments served by pnpr must advertise the proxy URL, or
/// tarball downloads bypass the emulated registry link.
#[must_use]
pub fn pnpr_command(port: u16, public_url: Option<&str>) -> Command {
    pnpr_command_with_binary(&pnpr_binary(), port, public_url)
}

/// Like [`pnpr_command`] but runs an explicit `pnpr` binary instead of the
/// one resolved from the workspace `target/` dir, so the integrated benchmark
/// can front each compared revision with a mock built from its own `pnpr`.
/// All revisions share the one runtime storage — serving a warm cache is
/// read-only, so concurrent mocks don't contend.
#[must_use]
pub fn pnpr_command_with_binary(bin: &Path, port: u16, public_url: Option<&str>) -> Command {
    pnpr_command_with_home(bin, port, public_url, None)
}

/// Configuration and storage for a mock built from an explicit revision.
#[derive(Debug, Clone, Copy)]
pub struct PnprMockConfig<'a> {
    pub home: &'a Path,
    /// Complete configuration in the revision's format, including its database URL.
    pub yaml: &'a str,
    /// The registries document to store in the home's metadata database, for
    /// a `pnpr` that reads its registries only from there. `None` for one
    /// whose `yaml` declares them.
    pub registries: Option<&'a Path>,
}

/// Like [`pnpr_command_with_binary`], keeping the hosted store and its
/// metadata database in `home` instead of the fixture build's when one is
/// given. The fixture build's store and database carry this checkout's
/// layout and metadata schema, which a `pnpr` built from another revision
/// may refuse; given a home of its own, that `pnpr` creates its own there
/// and serves everything it proxies, sharing only the proxy cache.
///
/// # Panics
///
/// Storing [`PnprMockConfig::registries`] fails.
#[must_use]
pub fn pnpr_command_with_home(
    bin: &Path,
    port: u16,
    public_url: Option<&str>,
    config: Option<PnprMockConfig<'_>>,
) -> Command {
    assert!(
        bin.is_file(),
        "pnpr binary not found at {bin:?} — \
         run `cargo build -p pnpr` before invoking the mock",
    );
    let seeded = seed_runtime_storage()
        .unwrap_or_else(|err| panic!("seed registry-mock fixtures into runtime storage: {err}"));
    if seeded > 0 {
        eprintln!("info: seeded {seeded} fixture file(s) into runtime storage");
    }
    let public_url = public_url.map_or_else(|| port_to_url(port), str::to_string);
    let mut cmd = command_without_pnpr_env(bin);
    // Front pnpr with the registry-mock config from `pnpr/.fixtures/config.yaml`
    // (the fixture namespace served from local hosted storage, everything else
    // proxied through the pattern-less npmjs upstream). We pin the runtime bits
    // the config file can't know about via CLI arguments.
    let (storage, database) = match &config {
        Some(config) => (config.home.join("storage"), None),
        None => (crate::runtime_hosted_storage(), Some(crate::runtime_metadata_path())),
    };
    let yaml = config.as_ref().map_or(pnpr_fixtures::MOCK_CONFIG_YAML, |config| config.yaml);
    let registries = config.and_then(|config| config.registries);
    let config = runtime_mock_config(database.as_deref(), yaml)
        .unwrap_or_else(|err| panic!("write the registry-mock config into runtime storage: {err}"));
    cmd.arg("--config")
        .arg(config)
        .arg("--storage")
        .arg(storage)
        .arg("--cache")
        .arg(crate::runtime_cache())
        .arg("--packument-ttl")
        .arg("31536000")
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--public-url")
        .arg(public_url.trim_end_matches('/'));
    if let Some(registries) = registries {
        import_registries(&cmd, registries);
    }
    cmd
}

/// Store the registries `document` declares in the metadata database of the
/// `pnpr` that `serve` starts, as its next revision, by running
/// `pnpr registries import` with `serve`'s program, arguments, and
/// environment. A server started on a database with no stored registries
/// stores the bundled default ones, so this runs before `serve` does.
///
/// # Panics
///
/// The import cannot be spawned or exits unsuccessfully.
pub fn import_registries(serve: &Command, document: &Path) {
    let mut import = Command::new(serve.get_program());
    for (name, value) in serve.get_envs() {
        match value {
            Some(value) => import.env(name, value),
            None => import.env_remove(name),
        };
    }
    if let Some(dir) = serve.get_current_dir() {
        import.current_dir(dir);
    }
    let output = import
        .args(serve.get_args())
        .arg("registries")
        .arg("import")
        .arg(document)
        .output()
        .unwrap_or_else(|err| panic!("spawn `pnpr registries import` for {document:?}: {err}"));
    assert!(
        output.status.success(),
        "`pnpr registries import` of {document:?} failed with {}:\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A [`Command`] of `bin` without the ambient `PNPR_*` variables. pnpr binds
/// each of its options to the variable named after it, so an ambient one — a
/// developer's `PNPR_CONFIG`, a stray `PNPR_DISABLE_REGISTRY` — would quietly
/// reconfigure the mock. The arguments the caller adds are the only thing
/// allowed to.
fn command_without_pnpr_env(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    for (name, _) in env::vars_os() {
        let key = name.to_string_lossy().to_ascii_uppercase();
        if key.starts_with("PNPR_") {
            cmd.env_remove(&name);
        }
    }
    cmd
}

/// Pin the metadata database beside the fixture build's hosted store.
/// The path includes the config's hash so concurrent configurations stay separate.
fn runtime_mock_config(database: Option<&Path>, config: &str) -> io::Result<PathBuf> {
    let yaml = match database {
        Some(database) => {
            let database = std::path::absolute(database)?;
            let mut yaml: serde_json::Value =
                serde_saphyr::from_str(config).map_err(io::Error::other)?;
            yaml["database"] = serde_json::json!({"url": format!("sqlite:{}", database.display())});
            serde_saphyr::to_string(&yaml).map_err(io::Error::other)?
        }
        None => config.to_string(),
    };
    let mut hasher = DefaultHasher::new();
    yaml.hash(&mut hasher);
    let mut path = std::path::absolute(crate::runtime_hosted_storage())?.into_os_string();
    path.push(format!("-{:016x}.yaml", hasher.finish()));
    let path = PathBuf::from(path);
    if fs::read_to_string(&path).is_ok_and(|existing| existing == yaml) {
        return Ok(path);
    }
    let directory = path.parent().expect("the config has a directory");
    fs::create_dir_all(directory)?;
    // One temporary name per write, so concurrent first launches in one
    // process never rename each other's file.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write = WRITES.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("yaml.{}-{write}", std::process::id()));
    fs::write(&temp, yaml)?;
    fs::rename(&temp, &path)?;
    Ok(path)
}
