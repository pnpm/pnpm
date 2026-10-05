#![cfg_attr(target_os = "wasi", feature(wasi_ext))]
#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
// A command's install future carries the engine's whole resolve-and-fetch
// graph; proving it `Send` walks deeper than rustc's default limit.
#![recursion_limit = "256"]

#[cfg(target_family = "wasm")]
extern crate pnpm_which as which;

#[cfg(target_family = "wasm")]
pub(crate) use pnpm_process as process;
#[cfg(not(target_family = "wasm"))]
pub(crate) use std::process;

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

mod boolean_negations;
mod boolean_values;
mod cargo_deps;
mod cargo_manifest;
mod checkbox_prompt;
#[cfg(target_family = "wasm")]
mod checkbox_terminal_wasm;
mod cli_args;
mod config_deps;
mod config_overrides;
mod confirm_prompt;
#[cfg(target_family = "wasm")]
mod dialoguer_wasm;
mod ecosystem_add;
mod ecosystem_install;
mod engine_pm;
mod executable_link;
mod fatal_error;
mod flag_relocation;
mod github_actions;
mod install_as_add;
mod leading_separator;
mod package_specifier;
mod parse_boundary;
mod path_env;
mod pm_prefix;
mod renamed_options;
mod shim_dispatch;
mod shorthands;
mod slot_lock;
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
    #[cfg(target_family = "wasm")]
    if let Err(error) = initialize_wasm_paths() {
        eprintln!("Failed to initialize pnpm host paths: {error}");
        return ExitCode::FAILURE;
    }
    // Runs before anything can print, so the first styled byte already
    // reaches a console that understands it; see `virtual_terminal`.
    virtual_terminal::enable();
    enable_tracing_by_env();
    install_report_handler();
    set_panic_hook();
    match run_on_big_stack(run_cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_fatal_error(&error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_family = "wasm")]
fn initialize_wasm_paths() -> std::io::Result<()> {
    let directory = std::env::var_os("PNPM_WASM_CWD")
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "PNPM_WASM_CWD is not set")
        })?;
    std::env::set_current_dir(directory)?;
    tempfile::env::override_temp_dir(&pnpm_fs::temp_dir())
        .map_err(|path| {
            std::io::Error::other(format!("Temporary directory already set to {}", path.display()))
        })
}

fn report_fatal_error(error: &miette::Report) {
    for cause in error.chain() {
        if let Some(pnpm_package_manager::InstallError::PeerDependencyIssues { rendered }) =
            cause.downcast_ref::<pnpm_package_manager::InstallError>()
        {
            if let Some(rendered) = rendered {
                eprint!("{rendered}");
            }
            return;
        }
    }
    if is_reported_error(error) {
        return;
    }
    if cli_args::reporter::selected_reporter() == cli_args::reporter::ReporterType::Ndjson {
        pnpm_reporter::NdjsonReporter::emit_fatal_error(&fatal_error::error_log(error));
    } else {
        eprintln!("Error: {error:?}");
    }
}

fn is_reported_error(error: &miette::Report) -> bool {
    error
        .code()
        .is_some_and(|code| {
            matches!(
                code.to_string().as_str(),
                "ERR_PNPM_DEDUPE_CHECK_ISSUES" | cli_args::recursive::NO_MATCHING_PROJECTS_CODE,
            )
        })
}

/// Parse and execute the CLI, including shim dispatch and startup fast paths.
fn run_cli() -> miette::Result<()> {
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
    let child_argv = argv_with_alias
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();
    // `pnpm pm <cmd>` is stripped before every other pass, so they all see
    // the command line the prefix stands for; the child argv above keeps
    // it, since a dispatched pnpm has to force the built-in too. See
    // `pm_prefix`.
    let (argv_with_alias, builtin_command_forced) = pm_prefix::strip_prefix(argv_with_alias);
    let (config_overrides, argv) = ConfigOverrides::extract(argv_with_alias);
    // A version spec (`pnpm with 10 <cmd>`) is left for the `with`
    // subcommand to handle.
    let argv = with_current::rewrite(argv)?;
    // The default reporter's `Done in ... using pacquet v<version>` footer needs
    // the version before the first event (including the fast path's).
    pnpm_default_reporter::set_package_version(pnpm_config::PNPM_VERSION);
    let (command, argv) = prepare_cli_argv(argv);
    let Some(mut args) = parse_or_answer(command, &argv, &child_argv, &config_overrides)? else {
        return Ok(());
    };
    configure_cli_args(&mut args)?;
    if dispatched_to_pinned_pnpm(&args, &config_overrides, &child_argv)? {
        return Ok(());
    }
    if args.run_completion_if_requested()? {
        return Ok(());
    }
    configure_rayon_pool();
    // An up-to-date `pacquet install` finishes here, without paying for
    // the runtime or the HTTP client.
    if args.finished_via_install_fast_path(&config_overrides) {
        return Ok(());
    }
    run_cli_command(args, &config_overrides, builtin_command_forced)
}

/// Parse argv into a command to run, or `None` when the command line was
/// already answered without one: the version was printed, or the pnpm the
/// project pins took a command line clap rejected.
fn parse_or_answer(
    command: clap::Command,
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<CliArgs>> {
    match parse_cli_args(command, argv.to_vec()) {
        Ok(args) => Ok(Some(args)),
        Err(err) if err.kind() == clap::error::ErrorKind::DisplayVersion => {
            print_version(argv, child_argv, config_overrides).map(|()| None)
        }
        Err(err) if err.kind() == clap::error::ErrorKind::UnknownArgument => {
            if dispatched_unparsed_to_pinned_pnpm(argv, child_argv, config_overrides)? {
                return Ok(None);
            }
            err.exit()
        }
        Err(err) => err.exit(),
    }
}

/// Parse argv, recording whether `--dir` or `-r` came from the command line.
fn parse_cli_args(command: clap::Command, argv: Vec<OsString>) -> Result<CliArgs, clap::Error> {
    command
        .try_get_matches_from(argv)
        .and_then(|matches| {
            let dir_from_command_line =
                matches.value_source("dir") == Some(clap::parser::ValueSource::CommandLine);
            let recursive_from_command_line =
                matches.value_source("recursive") == Some(clap::parser::ValueSource::CommandLine);
            CliArgs::from_arg_matches(&matches)
                .map(|args| CliArgs {
                    paths: crate::cli_args::cli_command::CliPathArgs {
                        dir_from_command_line,
                        ..args.paths
                    },
                    workspace: crate::cli_args::cli_command::CliWorkspaceArgs {
                        recursive_from_command_line,
                        ..args.workspace
                    },
                    ..args
                })
        })
}

/// pnpm prints the bare version, not clap's `pnpm <version>` rendering —
/// and a project that pins another pnpm answers for itself first.
fn print_version(
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<()> {
    // The version is the command's output, so every warning the checks
    // below raise belongs on stderr, leaving stdout a bare version string.
    pnpm_default_reporter::use_stderr();
    if pinned_pnpm_printed_the_version(argv, child_argv, config_overrides)? {
        return Ok(());
    }
    println!("{}", pnpm_config::PNPM_VERSION);
    Ok(())
}

/// Whether the pinned pnpm answered `--version` for this one. Installing
/// that pnpm, and recording the pin, both write, and a sandbox with a
/// read-only home has nowhere to write — printing a version has to work
/// there too, so the failure is reported and the running version answers.
/// The checks themselves still fail the command: a project pinned to
/// another package manager is not something a version string can stand in
/// for.
fn pinned_pnpm_printed_the_version(
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<bool> {
    let Some(plan) =
        cli_args::pre_command::pre_command_plan_for_version_flag(argv, config_overrides)?
    else {
        return Ok(false);
    };
    block_on_runtime("pacquet-pre-command", cli_args::pre_command::execute_plan(plan, child_argv))
        .or_else(|error| {
            cli_args::pre_command::warn_pinned_pnpm_unusable(&error);
            Ok(false)
        })
}

/// Whether the pnpm the project pins took the command. When it did, it has
/// already run to completion and this process has nothing left to do.
fn dispatched_to_pinned_pnpm(
    args: &CliArgs,
    config_overrides: &ConfigOverrides,
    child_argv: &[OsString],
) -> miette::Result<bool> {
    let plan = cli_args::pre_command::pre_command_plan(args, config_overrides)?;
    execute_pre_command_plan(plan, child_argv)
}

/// Whether the pnpm the project pins took a command line this one rejected.
/// A newer or older pnpm may accept an option this one does not know.
fn dispatched_unparsed_to_pinned_pnpm(
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<bool> {
    let plan = cli_args::pre_command::switch_plan_for_unparsed_argv(argv, config_overrides)?;
    execute_pre_command_plan(plan, child_argv)
}

fn execute_pre_command_plan(
    plan: Option<cli_args::pre_command::PreCommandPlan>,
    child_argv: &[OsString],
) -> miette::Result<bool> {
    let Some(plan) = plan else {
        return Ok(false);
    };
    block_on_runtime("pacquet-pre-command", cli_args::pre_command::execute_plan(plan, child_argv))
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

/// Poll the future on a fresh runtime with platform-independent stack headroom.
/// Propagate its result or panic to the caller.
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
    let exe = pnpm_executor::current_executable().ok();
    let exe_name = exe
        .as_deref()
        .and_then(Path::file_stem)
        .map(|stem| stem.to_string_lossy());
    inject_alias_subcommand(exe_name.as_deref(), argv)
}

/// Insert a leading `dlx` token after the program name when `exe_name` is a
/// `pnpx`/`pnx` alias. Split out from [`argv_with_alias_subcommand`] so the
/// argv rewrite is unit-testable without depending on `current_exe`.
fn inject_alias_subcommand(exe_name: Option<&str>, mut argv: Vec<OsString>) -> Vec<OsString> {
    if exe_name.is_some_and(pnpm_executor::is_pnpx_alias) {
        argv.insert(argv.len().min(1), OsString::from("dlx"));
    }
    argv
}

/// Size rayon's global pool with [`rayon_pool_size`].
///
/// Must run before anything touches rayon. The first parallel iterator
/// builds the global pool at rayon's default size, after which this
/// call can no longer size it, so debug builds assert that it did. The
/// repeat-install fast path uses rayon for workspace discovery.
///
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
/// actually schedule onto our cores.
fn configure_rayon_pool() {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return;
    }
    let parallelism = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let built = rayon::ThreadPoolBuilder::new()
        .num_threads(rayon_pool_size(parallelism, RAYON_THREADS_PER_CORE))
        .build_global();
    let configured = built.is_ok();
    debug_assert!(configured, "rayon's global pool was built before it was configured: {built:?}");
}

/// `threads_per_core × parallelism`, kept between [`MIN_RAYON_THREADS`]
/// and [`MAX_RAYON_THREADS`]. See [`RAYON_THREADS_PER_CORE`] for the
/// multiplier. The link phase is dominated by clonefile /
/// hardlink syscalls that block the calling thread on the kernel's
/// metadata journal, not by CPU work, so oversubscribing CPUs gives
/// more in-flight syscalls and a higher effective throughput.
/// Empirically sweeping 4-200 threads on a 1352-package warm install
/// on macOS APFS, 2× was the knee — fewer threads underutilize the
/// journal, way more (100+) loses to context switching and per-thread
/// fixed costs (`user` time scales linearly past 50 without any
/// wall-time payoff). That knee holds up to the ceiling below. Past 16
/// threads, 2× no longer pays off.
///
/// **Floor of 4 threads is intentional.** A 1-2-CPU CI runner left
/// at `2 × parallelism` would be capped to 2-4 rayon threads, and
/// at that point we go back to the original "one rayon thread is
/// blocked on a `clonefile` while the next fully-ready snapshot
/// can't even start" pattern that the 2× tuning is trying to
/// avoid. The kernel metadata journal is the bottleneck even on
/// small hosts, so a small intentional oversubscription
/// (`max(4, 2 × parallelism)`) is a better trade than respecting the
/// quota literally.
///
/// **Ceiling of 16 threads.** Past 16, the extra workers add system
/// time without shortening the install. A warm-install sweep (nuxt, next, nitro
/// fixtures) found no machine where 2× beat 16 threads: a 32-vCPU
/// Linux runner kept its wall time and halved its system time, a
/// 16-vCPU Windows runner got 10% faster, and a 10-core M1 Max, the
/// one host where 2× beat 8 threads, was unchanged at 16
/// (pnpm/tasks#51). A ceiling of 8 cut Linux system time further and
/// sped up Windows, but cost that Mac 19%.
fn rayon_pool_size(parallelism: usize, threads_per_core: usize) -> usize {
    parallelism.saturating_mul(threads_per_core).clamp(MIN_RAYON_THREADS, MAX_RAYON_THREADS)
}

/// Two threads per core, except on Windows, where the sweeps mostly
/// favoured one (pnpm/tasks#52). Warm frozen installs were 4-5% faster
/// at 1× on 4- and 8-vCPU runners. A fresh install of the 1352-package
/// benchmark fixture took 3.9 s at 1× and 4.5 s at 2× on a 4-vCPU
/// runner, and was even on an 8-vCPU one. A 16-vCPU runner was fastest
/// at 4 threads, below what this multiplier gives it.
const RAYON_THREADS_PER_CORE: usize = if cfg!(windows) { 1 } else { 2 };
const MIN_RAYON_THREADS: usize = 4;
const MAX_RAYON_THREADS: usize = 16;

#[cfg(test)]
mod tests;

/// Normalize pnpm argument syntax before passing it to clap.
fn prepare_cli_argv(argv: Vec<OsString>) -> (clap::Command, Vec<OsString>) {
    let command = with_boolean_negations(CliArgs::command());
    let argv = shorthands::expand_universal_shorthands(&command, argv);
    let argv = boolean_values::resolve_boolean_values(argv);
    let argv = renamed_options::drop_shadowed_aliases(&command, argv);
    let argv = relocate_pre_subcommand_flags(&command, argv);
    let argv = install_as_add::rewrite(&command, argv);
    let argv = leading_separator::preserve_leading_separator(argv);
    (command, argv)
}

fn configure_cli_args(args: &mut CliArgs) -> miette::Result<()> {
    cli_args::reporter::select_reporter(args.effective_reporter());
    fatal_error::set_prefix(&args.paths.dir);
    if let Err(err) = args.validate_command_scoped_global_options() {
        err.exit();
    }
    args.apply_parallel_run_options();
    args.promote_recursive_for_filter();
    args.apply_local_prefix()?;
    args.apply_workspace_root()?;
    args.ignore_workspace_for_global_config_read();
    args.promote_recursive_by_default();
    args.configure_reporter();
    fatal_error::set_prefix(&args.paths.dir);
    cli_args::sudo_guard::check_sudo(&args.command)?;
    Ok(())
}

fn run_cli_command(
    args: CliArgs,
    config_overrides: &ConfigOverrides,
    builtin_command_forced: bool,
) -> miette::Result<()> {
    // Arm Windows process-tree cleanup until the command succeeds.
    let job_guard = pnpm_executor::arm_process_tree_cleanup();
    let result =
        block_on_runtime("pacquet-main", args.run(config_overrides, builtin_command_forced));
    if result.is_ok()
        && let Some(job_guard) = job_guard
    {
        job_guard.disarm();
    }
    result
}
