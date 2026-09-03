mod boolean_negations;
mod checkbox_prompt;
mod cli_args;
mod config_deps;
mod config_overrides;
mod engine_pm;
mod executable_link;
mod flag_relocation;
mod github_actions;
mod install_as_add;
mod leading_separator;
mod parse_boundary;
mod path_env;
mod pm_prefix;
mod renamed_options;
mod shim_dispatch;
mod shorthands;
mod state;
mod virtual_terminal;
mod with_current;

use boolean_negations::with_boolean_negations;
use clap::{CommandFactory, FromArgMatches};
use cli_args::CliArgs;
use config_overrides::ConfigOverrides;
use flag_relocation::relocate_pre_subcommand_flags;
use miette::set_panic_hook;
use pnpm_diagnostics::{enable_tracing_by_env, install_report_handler};
use state::State;
use std::{ffi::OsString, future::Future, path::Path, process::ExitCode};

pub fn main() -> ExitCode {
    // Runs before anything can print, so the first styled byte already
    // reaches a console that understands it; see `virtual_terminal`.
    virtual_terminal::enable();
    enable_tracing_by_env();
    install_report_handler();
    set_panic_hook();
    // The synchronous startup in `run_cli` — building the negation-augmented
    // clap command (a recursive walk over every subcommand), relocating
    // flags, and parsing argv — has a deep enough call chain to overflow
    // Windows' 1 MiB default main-thread stack (Linux/macOS default to
    // 8 MiB). `block_on_runtime` already moves the command body onto a
    // roomy thread; run the parsing startup that precedes it on one too so
    // the whole path has uniform headroom.
    match run_on_big_stack(run_cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if !is_reported_error(&error) {
                eprintln!("Error: {error:?}");
            }
            ExitCode::FAILURE
        }
    }
}

fn is_reported_error(error: &miette::Report) -> bool {
    error.code().is_some_and(|code| {
        matches!(
            code.to_string().as_str(),
            "ERR_PNPM_DEDUPE_CHECK_ISSUES"
                | "ERR_PNPM_PEER_DEP_ISSUES"
                | cli_args::recursive::NO_MATCHING_PROJECTS_CODE,
        )
    })
}

/// Build the CLI, parse argv, take any early-return fast path, then execute
/// the command. Split out of [`main`] so the whole path runs on a
/// [`MAIN_STACK_SIZE`] thread; see the call site.
fn run_cli() -> miette::Result<()> {
    // Extract pnpm's `--config.<key>=<value>` tokens before clap sees
    // argv. Clap can't parse a dotted-key flag whose right-hand name is
    // arbitrary, so a `--config.registry=...` from pnpm's forwarded flags
    // would otherwise error out as "unexpected argument". Each extracted
    // token is layered onto `Config` after `.npmrc` / yaml run.
    let argv: Vec<OsString> = std::env::args_os().collect();
    // A context-aware global shim is this executable launched under the
    // shim's name, so dispatch runs on the raw argv before any rewriting
    // or clap machinery below: a shim named like an alias must not have
    // the alias subcommand injected into the arguments it forwards.
    if let Some(exit_code) = shim_dispatch::try_dispatch(&argv) {
        #[expect(
            clippy::exit,
            reason = "the shim dispatcher propagates the dispatched command's exit status"
        )]
        std::process::exit(exit_code);
    }
    let argv_with_alias = argv_with_alias_subcommand(argv);
    let child_argv = argv_with_alias.iter().skip(1).cloned().collect::<Vec<_>>();
    // `pnpm pm <cmd>` is stripped before every other pass, so they all see
    // the command line the prefix stands for; the child argv above keeps
    // it, since a dispatched pnpm has to force the built-in too. See
    // `pm_prefix`.
    let (argv_with_alias, builtin_command_forced) = pm_prefix::strip_prefix(argv_with_alias);
    let (config_overrides, argv) = ConfigOverrides::extract(argv_with_alias);
    // `pnpm with current <cmd>` is sugar for running `<cmd>` in-process with
    // the packageManager / devEngines check disabled; rewrite argv before
    // clap parses it. A version spec (`pnpm with 10 <cmd>`) is left for the
    // `with` subcommand to handle.
    let argv = with_current::rewrite(argv)?;
    // The default reporter's `Done in ... using pacquet v<version>` footer needs
    // the version before the first event (including the fast path's).
    pnpm_default_reporter::set_package_version(pnpm_config::PNPM_VERSION);
    // Parse through a command augmented with a `--no-<flag>` negation for
    // every boolean flag, so pnpm's forwarded negations (`--no-frozen-lockfile`,
    // etc.) parse the same way nopt accepts them upstream. See `boolean_negations`.
    let command = with_boolean_negations(CliArgs::command());
    // pnpm expands universal shorthands (`--silent` / `-s` →
    // `--reporter=silent`) over argv before parsing; mirror that so they
    // work with every command. See `shorthands`.
    let argv = shorthands::expand_universal_shorthands(&command, argv);
    // npm's spellings of two of pnpm's options (`--prefix`, `--store`) are
    // hidden clap aliases; pnpm additionally lets the canonical spelling win
    // when a command line uses both. See `renamed_options`.
    let argv = renamed_options::drop_shadowed_aliases(&command, argv);
    // pnpm's option parser is position-independent; move subcommand
    // options written before the subcommand to after it so clap agrees.
    // See `flag_relocation`.
    let argv = relocate_pre_subcommand_flags(&command, argv);
    // `pnpm install <pkg>` is pnpm's spelling of `pnpm add <pkg>`; clap's
    // `install` takes no package name, so rename the subcommand token.
    // See `install_as_add`.
    let argv = install_as_add::rewrite(&command, argv);
    // A command whose arguments begin at a `--` would otherwise lose it to
    // clap's escape handling. See `leading_separator`.
    let argv = leading_separator::preserve_leading_separator(argv);
    let mut args = match command
        .try_get_matches_from(argv.clone())
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
    {
        Ok(args) => args,
        // pnpm prints the bare version, not clap's "pnpm <version>" rendering.
        Err(err) if err.kind() == clap::error::ErrorKind::DisplayVersion => {
            if let Some(plan) =
                cli_args::pre_command::pre_command_plan_for_version_flag(&argv, &config_overrides)?
                && block_on_runtime(
                    "pacquet-pre-command",
                    cli_args::pre_command::execute_plan(plan, &child_argv),
                )?
            {
                return Ok(());
            }
            println!("{}", pnpm_config::PNPM_VERSION);
            return Ok(());
        }
        Err(err) => err.exit(),
    };
    if let Err(err) = args.validate_command_scoped_global_options() {
        err.exit();
    }
    args.apply_parallel_run_options();
    args.promote_recursive_for_filter();
    args.apply_workspace_root()?;
    args.promote_recursive_by_default();
    args.configure_reporter();
    cli_args::sudo_guard::check_sudo(&args.command)?;
    if let Some(plan) = cli_args::pre_command::pre_command_plan(&args, &config_overrides)?
        && block_on_runtime(
            "pacquet-pre-command",
            cli_args::pre_command::execute_plan(plan, &child_argv),
        )?
    {
        return Ok(());
    }
    // An up-to-date `pacquet install` finishes here, without paying for
    // the runtime, the HTTP client, or any worker threads.
    if args.finished_via_install_fast_path(&config_overrides) {
        return Ok(());
    }
    if args.run_completion_if_requested()? {
        return Ok(());
    }
    // Arm Windows process-tree cleanup until the command succeeds.
    let job_guard = pnpm_executor::arm_process_tree_cleanup();
    configure_rayon_pool();
    // `block_on` polls the command future on the calling thread, and the
    // install pipeline has a deep synchronous call chain whose stack frames
    // overflow Windows' 1 MiB default main-thread stack (Linux and macOS
    // default to 8 MiB, so the limit trips on Windows first). Run it on a
    // thread with a generous, platform-uniform stack instead of the OS
    // default main-thread stack.
    let result =
        block_on_runtime("pacquet-main", args.run(&config_overrides, builtin_command_forced));
    if result.is_ok()
        && let Some(job_guard) = job_guard
    {
        job_guard.disarm();
    }
    result
}

/// Stack size for the thread the command runs on. Generous headroom over
/// the 8 MiB Linux/macOS default so the deep install call chain has the
/// same room on every platform, including Windows (1 MiB default).
const MAIN_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Run a synchronous closure on a fresh [`MAIN_STACK_SIZE`] thread and
/// return its result, re-raising any panic on the caller. Gives the CLI
/// startup the same stack headroom [`block_on_runtime`] gives the command,
/// without a tokio runtime (so it can wrap code that later builds one).
fn run_on_big_stack<Work, Output>(work: Work) -> Output
where
    Work: FnOnce() -> Output + Send,
    Output: Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("pacquet-startup".to_string())
            .stack_size(MAIN_STACK_SIZE)
            .spawn_scoped(scope, work)
            .expect("spawn the pacquet startup thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

fn block_on_runtime<Work, Output>(thread_name: &str, work: Work) -> Output
where
    Work: Future<Output = Output> + Send,
    Output: Send,
{
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name(thread_name.to_string())
            .stack_size(MAIN_STACK_SIZE)
            .spawn_scoped(scope, move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("build the tokio runtime")
                    // Boxed for `clippy::large_futures`: the command future
                    // exceeds the lint's stack-size threshold.
                    .block_on(Box::pin(work))
            })
            .expect("spawn the pacquet runtime thread");
        handle
            .join()
            // Re-raise a panic from the worker on this thread so the process
            // still aborts with the original message and backtrace.
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// Process argv with a leading `dlx` injected when launched as `pnpx`/`pnx`
/// (shorthand for `pnpm dlx`), mirroring pnpm's `buildArgv`. Only the Windows
/// hardlink aliases rely on this — the Unix alias scripts inject `dlx`
/// themselves — and there `current_exe` is the only signal of the launch name.
fn argv_with_alias_subcommand(argv: Vec<OsString>) -> Vec<OsString> {
    let exe = std::env::current_exe().ok();
    let exe_name =
        exe.as_deref().and_then(Path::file_stem).map(|stem| stem.to_string_lossy().to_lowercase());
    inject_alias_subcommand(exe_name.as_deref(), argv)
}

/// Insert a leading `dlx` token after the program name when `exe_name` is a
/// `pnpx`/`pnx` alias. Split out from [`argv_with_alias_subcommand`] so the
/// argv rewrite is unit-testable without depending on `current_exe`.
fn inject_alias_subcommand(exe_name: Option<&str>, mut argv: Vec<OsString>) -> Vec<OsString> {
    if matches!(exe_name, Some("pnpx" | "pnx")) {
        argv.insert(argv.len().min(1), OsString::from("dlx"));
    }
    argv
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

#[cfg(test)]
mod tests;
