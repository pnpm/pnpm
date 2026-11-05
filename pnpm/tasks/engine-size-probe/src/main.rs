//! The smallest executable that embeds pnpm's install engine.
//!
//! `engine-size` builds and measures it. It reaches the engine only through
//! the library API an embedding application uses, so the code it links is
//! the code such an application links. Keep it that way: an extra crate or
//! feature here inflates every size the benchmark reports.
//!
//! Usage: `engine-size-probe <project-dir> [--frozen-lockfile]`.

use miette::{IntoDiagnostic, WrapErr};
use pnpm_config::{Config, Host};
use pnpm_lockfile::{LazyLockfile, MaybeLazyLockfile};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{Install, ResolvedPackages};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_tarball::MemCache;
use std::{path::PathBuf, process::ExitCode, sync::Arc};

/// The CLI runs installs on a thread with this stack size, because the
/// install call chain overflows a default stack on some platforms.
const INSTALL_STACK_SIZE: usize = 32 * 1024 * 1024;

struct ProbeArgs {
    project_dir: PathBuf,
    frozen_lockfile: bool,
}

fn main() -> ExitCode {
    let outcome = parse_args()
        .and_then(|args| {
            std::thread::Builder::new()
                .stack_size(INSTALL_STACK_SIZE)
                .spawn(move || install(&args))
                .into_diagnostic()?
                .join()
                .map_err(|_| miette::miette!("the install thread panicked"))?
        });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            #[expect(clippy::print_stderr, reason = "developer tool, not part of the pnpm binary")]
            {
                eprintln!("{error:?}");
            }
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> miette::Result<ProbeArgs> {
    let mut project_dir = None;
    let mut frozen_lockfile = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--frozen-lockfile" {
            frozen_lockfile = true;
        } else if project_dir.is_none() {
            project_dir = Some(PathBuf::from(arg));
        } else {
            miette::bail!("unexpected argument {arg:?}");
        }
    }
    let project_dir =
        project_dir.ok_or_else(|| miette::miette!("usage: engine-size-probe <project-dir>"))?;
    Ok(ProbeArgs { project_dir, frozen_lockfile })
}

fn install(args: &ProbeArgs) -> miette::Result<()> {
    pnpm_package_manager::configure_rayon_pool();
    let dir = &args.project_dir;
    let config: &'static Config = Config::default()
        .current::<Host>(dir)
        .wrap_err("load the configuration")?
        .leak();
    let http_client = Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .wrap_err("create the HTTP client")?
        .with_max_sockets_per_host(config.max_sockets),
    );
    let manifest = PackageManifest::from_path(dir.join("package.json"))?;
    let lockfile_dir = config.lockfile_dir_for(dir).to_path_buf();
    let lockfile_path = lockfile_dir.join(config.wanted_lockfile_name());
    let lockfile = LazyLockfile::deferred(lockfile_dir, config.wanted_lockfile_selection());
    let resolved_packages = ResolvedPackages::new();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;
    runtime.block_on(async {
        let mut install = Install::new(
            Arc::new(MemCache::new()),
            &resolved_packages,
            (&http_client, Arc::clone(&http_client)),
            config,
            &manifest,
            MaybeLazyLockfile::Lazy(&lockfile),
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        );
        install.lockfile_policy.frozen = args.frozen_lockfile;
        install.context.lockfile_path = Some(&lockfile_path);
        install.run::<SilentReporter>().await.wrap_err("install")
    })
}
