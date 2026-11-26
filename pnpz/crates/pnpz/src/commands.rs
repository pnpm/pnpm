use crate::{
    binary_cache::{
        BinaryCache,
        CACHE_URL_ENV,
        DEFAULT_CACHE_URL,
        TRUSTED_KEYS_ENV,
    },
    catalog::{
        CATALOG_URL_ENV,
        Catalog,
        DEFAULT_CATALOG_URL,
    },
    environment::{
        describe,
        search_path,
    },
    install::{
        install_closure,
        missing_paths,
    },
    lockfile::{
        LOCKFILE_NAME,
        Lockfile,
    },
    manifest::{
        MANIFEST_FILE_NAME,
        Manifest,
    },
    project::{
        is_up_to_date,
        lock,
        wanted_systems,
    },
    store::Store,
    system::host_system,
};
use command_extra::CommandExtra as _;
use miette::{
    Context as _,
    IntoDiagnostic as _,
    miette,
};
use pnpm_diagnostics::eprintln;
use pnpm_network::ThrottledClient;
use pnpz_nix_store::{
    NIXOS_CACHE_KEY,
    TrustedKey,
};
use std::{
    env,
    ffi::OsString,
    os::unix::process::CommandExt as _,
    path::{
        Path,
        PathBuf,
    },
    process::Command,
    sync::Arc,
    time::Instant,
};

/// What every command works on: a project directory, the store, and the
/// platform packages are installed for.
struct Project {
    directory: PathBuf,
    store: Store,
    system: String,
}

impl Project {
    fn open() -> miette::Result<Self> {
        Ok(Self {
            directory: env::current_dir().into_diagnostic()?,
            store: Store::discover()?,
            system: host_system()?,
        })
    }
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn remotes() -> miette::Result<(Catalog, BinaryCache)> {
    let client = Arc::new(ThrottledClient::default());
    let trusted_keys = env_or(TRUSTED_KEYS_ENV, NIXOS_CACHE_KEY)
        .split_ascii_whitespace()
        .map(TrustedKey::parse)
        .collect::<Result<_, _>>()?;
    let catalog = Catalog::new(Arc::clone(&client), &env_or(CATALOG_URL_ENV, DEFAULT_CATALOG_URL));
    let cache = BinaryCache::new(client, &env_or(CACHE_URL_ENV, DEFAULT_CACHE_URL), trusted_keys);
    Ok((catalog, cache))
}

pub fn add(specs: &[String]) -> miette::Result<()> {
    let project = Project::open()?;
    let mut manifest = Manifest::load(&project.directory)?;
    for spec in specs {
        manifest.add(spec)?;
    }
    block_on(synchronize(&project, &manifest, Frozen::No))?;
    manifest.save(&project.directory)?;
    Ok(())
}

pub fn install(frozen: bool) -> miette::Result<()> {
    let project = Project::open()?;
    let manifest = Manifest::load(&project.directory)?;
    let frozen = if frozen { Frozen::Yes } else { Frozen::No };
    block_on(synchronize(&project, &manifest, frozen)).map(drop)
}

/// Runs `future` on a runtime made for it. Commands that end in `exec` call
/// this only when they have something to download, so that the common case
/// starts no threads.
fn block_on<Output>(
    future: impl Future<Output = miette::Result<Output>>,
) -> miette::Result<Output> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(future)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Frozen {
    Yes,
    No,
}

/// Brings the lockfile up to date with `manifest`, unless `frozen`, and the
/// store up to date with the lockfile.
async fn synchronize(
    project: &Project,
    manifest: &Manifest,
    frozen: Frozen,
) -> miette::Result<Lockfile> {
    let started = Instant::now();
    let (catalog, cache) = remotes()?;
    let systems = wanted_systems(manifest, &project.system);
    let lockfile = match Lockfile::load(&project.directory)? {
        Some(lockfile) if is_up_to_date(manifest, &lockfile, &systems) => lockfile,
        _ if frozen == Frozen::Yes => {
            return Err(miette!(
                code = "ERR_PNPZ_OUTDATED_LOCKFILE",
                "{LOCKFILE_NAME} is not up to date with {MANIFEST_FILE_NAME}",
            ));
        }
        previous => {
            let lockfile = lock(manifest, previous.as_ref(), &systems, (&catalog, &cache)).await?;
            lockfile.save(&project.directory)?;
            lockfile
        }
    };
    let closure = lockfile.closures
        .get(&project.system)
        .ok_or_else(|| {
            miette!(
                code = "ERR_PNPZ_SYSTEM_NOT_LOCKED",
                "{MANIFEST_FILE_NAME} does not list {}",
                project.system
            )
        })?;
    eprintln!("Packages: {}", describe(&lockfile, &project.system).join(", "));
    let summary = install_closure(&cache, &project.store, closure, |path, locked| {
        eprintln!("  + {} ({})", path.name(), megabytes(locked.nar_size));
    })
    .await?;
    eprintln!(
        "Store paths: {} fetched ({}), {} reused. Done in {:.1}s",
        summary.fetched_paths,
        megabytes(summary.fetched_bytes),
        summary.reused_paths,
        started.elapsed().as_secs_f64(),
    );
    Ok(lockfile)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

pub fn run(command: &[OsString]) -> miette::Result<()> {
    let project = Project::open()?;
    let lockfile = ready_lockfile(&project)?;
    exec_in_environment(&project, &lockfile, command)
}

pub fn shell() -> miette::Result<()> {
    let shell = env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    run(&[shell])
}

/// The project's lockfile, after making sure the store holds what it locks.
///
/// When nothing is missing this touches neither the network nor an async
/// runtime, which keeps `pnpz run` cheap enough to wrap every command.
fn ready_lockfile(project: &Project) -> miette::Result<Lockfile> {
    let manifest = Manifest::load(&project.directory)?;
    let systems = wanted_systems(&manifest, &project.system);
    let installed = Lockfile::load(&project.directory)?
        .filter(|lockfile| {
            is_up_to_date(&manifest, lockfile, &systems)
                && lockfile.closures
                    .get(&project.system)
                    .is_some_and(|closure| missing_paths(&project.store, closure).is_empty())
        });
    match installed {
        Some(lockfile) => Ok(lockfile),
        None => block_on(synchronize(project, &manifest, Frozen::No)),
    }
}

/// Replaces this process with `command`, run where `/nix/store` holds the
/// project's packages and `PATH` finds them.
fn exec_in_environment(
    project: &Project,
    lockfile: &Lockfile,
    command: &[OsString],
) -> miette::Result<()> {
    let process = if project.store.is_native() {
        Command::new(&command[0])
    } else {
        Command::new(env::current_exe().into_diagnostic()?)
            .with_arg("__enter")
            .with_arg(project.store.root())
            .with_arg(&command[0])
    };
    let process = process
        .with_args(&command[1..])
        .with_env("PATH", search_path(lockfile, &project.system, &project.store));
    exec(process, command)
}

/// Mounts the store at `store_root` on `/nix`, then replaces this process
/// with `command`.
#[cfg(target_os = "linux")]
pub fn enter(store_root: &Path, command: &[OsString]) -> miette::Result<()> {
    crate::sandbox::enter(store_root)?;
    exec(Command::new(&command[0]).with_args(&command[1..]), command)
}

#[cfg(not(target_os = "linux"))]
pub fn enter(store_root: &Path, _command: &[OsString]) -> miette::Result<()> {
    let store_root = store_root.display();
    Err(miette!(
        code = "ERR_PNPZ_STORE_NOT_AT_NIX",
        help = "create a /nix/store directory that you own, and pnpz will install there",
        "the store at {store_root} is not at /nix, and this platform cannot mount it there without root",
    ))
}

/// Replaces this process with `process`. Returns only when that fails.
fn exec(mut process: Command, command: &[OsString]) -> miette::Result<()> {
    let program = command[0].display();
    Err(process.exec())
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to run {program}"))
}
