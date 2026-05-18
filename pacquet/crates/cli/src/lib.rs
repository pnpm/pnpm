mod cli_args;
mod state;

use clap::Parser;
use cli_args::CliArgs;
use miette::set_panic_hook;
use pacquet_diagnostics::enable_tracing_by_env;
use state::State;

pub async fn main() -> miette::Result<()> {
    enable_tracing_by_env();
    set_panic_hook();
    // Run argument parsing *before* sizing the rayon pool so
    // `pacquet --help` / `--version` (and any clap parse error) exit
    // without spinning up worker threads. `clap::Parser::parse` calls
    // `std::process::exit` on those paths, so we never reach
    // `configure_rayon_pool` for them (Copilot review on #292).
    let args = CliArgs::parse();
    configure_rayon_pool();
    args.run().await
}

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
/// Use [`std::thread::available_parallelism`] rather than the
/// workspace's existing `num_cpus::get()` so cgroup / CPU-quota
/// limits in containers and CI runners are respected — `num_cpus`
/// reports the host's logical CPU count, which on a quota-limited
/// runner can spin up far more rayon threads than the kernel will
/// actually schedule onto our cores (Copilot review on #292).
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
/// Honours an explicit `RAYON_NUM_THREADS` env var by skipping our
/// override (rayon's `build_global` errors if a pool is already set,
/// but env vars don't pre-init it — so we just apply a smaller
/// override only when nothing else has been configured). Best-effort:
/// if another part of the binary already initialised the pool, leave
/// it alone.
fn configure_rayon_pool() {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return;
    }
    let n = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .saturating_mul(2)
        // `.max(4)` is an intentional minimum: even on quota-limited
        // 1-2-CPU runners, dropping below 4 puts us back into the
        // "rayon worker stalls on `clonefile` while the next snapshot
        // can't start" regime. See the function-level doc.
        .max(4);
    let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
}
