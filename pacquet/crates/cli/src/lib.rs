mod cli_args;
mod config_deps;
mod config_overrides;
mod job_control;
mod state;

use clap::Parser;
use cli_args::CliArgs;
use config_overrides::ConfigOverrides;
use miette::set_panic_hook;
use pacquet_diagnostics::enable_tracing_by_env;
use state::State;

pub fn main() -> miette::Result<()> {
    enable_tracing_by_env();
    set_panic_hook();
    // Extract pnpm's `--config.<key>=<value>` tokens before clap sees
    // argv. Clap can't parse a dotted-key flag whose right-hand name is
    // arbitrary, so a `--config.registry=...` from pnpm's forwarded flags
    // would otherwise error out as "unexpected argument". Each extracted
    // token is layered onto `Config` after `.npmrc` / yaml run.
    let (config_overrides, argv) = ConfigOverrides::extract(std::env::args_os());
    // The default reporter's `Done in ... using pacquet v<version>` footer needs
    // the version before the first event (including the fast path's).
    pacquet_default_reporter::set_package_version(pacquet_config::PACQUET_VERSION);
    let args = CliArgs::parse_from(argv);
    // An up-to-date `pacquet install` finishes here, without paying for
    // the runtime, the HTTP client, or any worker threads.
    if args.finished_via_install_fast_path(&config_overrides) {
        return Ok(());
    }
    // Tie any child pacquet spawns (lifecycle scripts and their descendants)
    // to this process so none are orphaned on Windows. Held until `main`
    // returns; see `job_control`.
    let _job_guard = job_control::setup();
    configure_rayon_pool();
    // `block_on` polls the command future on the calling thread, and the
    // install pipeline has a deep synchronous call chain whose stack frames
    // overflow Windows' 1 MiB default main-thread stack (Linux and macOS
    // default to 8 MiB, so the limit trips on Windows first). Run it on a
    // thread with a generous, platform-uniform stack instead of the OS
    // default main-thread stack.
    std::thread::Builder::new()
        .name("pacquet-main".to_string())
        .stack_size(MAIN_STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build the tokio runtime")
                // Boxed for `clippy::large_futures`: the command future
                // exceeds the lint's stack-size threshold.
                .block_on(Box::pin(args.run(&config_overrides)))
        })
        .expect("spawn the pacquet-main thread")
        .join()
        // Re-raise a panic from the worker on this thread so the process
        // still aborts with the original message and backtrace.
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

/// Stack size for the thread the command runs on. Generous headroom over
/// the 8 MiB Linux/macOS default so the deep install call chain has the
/// same room on every platform, including Windows (1 MiB default).
const MAIN_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Size rayon's global pool at `2 × available_parallelism`. The link
/// phase is dominated by clonefile / hardlink syscalls that block the
/// calling thread on the kernel's metadata journal, not by CPU work,
/// so oversubscribing CPUs gives more in-flight syscalls and a higher
/// effective throughput. Empirically sweeping 4-200 threads on a
/// 1352-package warm install on macOS APFS, 2× was the knee — fewer
/// threads underutilize the journal, way more (100+) loses to context
/// switching and per-thread fixed costs (`user` time scales linearly
/// past 50 without any wall-time payoff).
///
/// Runs after the repeat-install fast path has declined, so commands
/// that never reach a parallel phase (`--help`, the "Already up to
/// date" short-circuit) skip the worker-thread spawn cost entirely.
/// Deliberately NOT communicated via the `RAYON_NUM_THREADS`
/// environment variable: a process-env write would leak into every
/// child the install spawns (lifecycle scripts, `node --version`,
/// git), and pnpm exposes no such variable to scripts. An explicit
/// `RAYON_NUM_THREADS` from the caller is honoured by skipping the
/// override.
///
/// Use [`std::thread::available_parallelism`] rather than the
/// workspace's existing `num_cpus::get()` so cgroup / CPU-quota
/// limits in containers and CI runners are respected — `num_cpus`
/// reports the host's logical CPU count, which on a quota-limited
/// runner can spin up far more rayon threads than the kernel will
/// actually schedule onto our cores (Copilot review on [#292]).
///
/// **Floor of 4 threads is intentional.** A 1-2-CPU CI runner left
/// at `2 × parallelism` would be capped to 2-4 rayon threads, and
/// at that point we go back to the original "one rayon thread is
/// blocked on a `clonefile` while the next fully-ready snapshot
/// can't even start" pattern that the 2× tuning is trying to
/// avoid. The kernel metadata journal is the bottleneck even on
/// small hosts, so a small intentional oversubscription
/// (max(4, 2 × parallelism)) is a better trade than respecting the
/// quota literally — Copilot's follow-up flagged the tension; we're
/// keeping the floor and documenting it explicitly.
///
/// Best-effort: if another part of the binary already initialised the
/// pool, leave it alone.
///
/// [#292]: https://github.com/pnpm/pacquet/pull/292
fn configure_rayon_pool() {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return;
    }
    let n = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .saturating_mul(2)
        // `.max(4)` is an intentional minimum: even on quota-limited
        // 1-2-CPU runners, dropping below 4 puts us back into the
        // "rayon worker stalls on `clonefile` while the next snapshot
        // can't start" regime. See the function-level doc.
        .max(4);
    let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
}
