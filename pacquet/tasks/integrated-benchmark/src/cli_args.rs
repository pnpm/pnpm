use clap::{Args, Parser, ValueEnum};
use pipe_trait::Pipe;
use std::{path::PathBuf, process::Command};

#[derive(Debug, Parser)]
pub struct CliArgs {
    /// Task to benchmark.
    #[clap(long, short, required_unless_present = "build_only", conflicts_with = "build_only")]
    pub scenario: Option<BenchmarkScenario>,

    /// Port of the local virtual registry.
    #[clap(long, short = 'p', default_value_t = 4873)]
    pub registry_port: u16,

    /// Automatically launch verdaccio if local registry doesn't response.
    #[clap(long, short = 'V')]
    pub verdaccio: bool,

    /// Path to the git repository of pacquet.
    #[clap(long, short = 'R', default_value = ".")]
    pub repository: PathBuf,

    /// Override default `package.json` and `pnpm-lock.yaml` by specifying the directory containing them.
    #[clap(long, short = 'D')]
    pub fixture_dir: Option<PathBuf>,

    /// Flags to pass to `hyperfine`.
    #[clap(flatten)]
    pub hyperfine_options: HyperfineOptions,

    /// Path to the work environment.
    #[clap(long, short, default_value = "bench-work-env")]
    pub work_env: PathBuf,

    /// Benchmark against pnpm.
    #[clap(long)]
    pub with_pnpm: bool,

    /// Build each revision without running the benchmark.
    #[clap(long)]
    pub build_only: bool,

    /// Branch name, tag name, or commit id of the pacquet repo.
    #[clap(required = true)]
    pub revisions: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BenchmarkScenario {
    /// Benchmark clean install without lockfile and without local cache.
    CleanInstall,
    /// Benchmark install with a frozen lockfile and without local cache.
    FrozenLockfile,
    /// Benchmark install with a frozen lockfile and a warm local store.
    FrozenLockfileHotCache,
}

impl BenchmarkScenario {
    /// Infer CLI arguments for the install command.
    pub fn install_args(self) -> impl IntoIterator<Item = &'static str> {
        match self {
            BenchmarkScenario::CleanInstall => Vec::new(),
            BenchmarkScenario::FrozenLockfile | BenchmarkScenario::FrozenLockfileHotCache => {
                vec!["--frozen-lockfile"]
            }
        }
    }

    /// Return `lockfile=true` or `lockfile=false` for use in generating `.npmrc`.
    pub fn npmrc_lockfile_setting(self) -> &'static str {
        match self {
            BenchmarkScenario::CleanInstall => "lockfile=false",
            BenchmarkScenario::FrozenLockfile | BenchmarkScenario::FrozenLockfileHotCache => {
                "lockfile=true"
            }
        }
    }

    /// Whether the lockfile is enabled for this scenario. Mirrored into
    /// `pnpm-workspace.yaml` alongside `.npmrc` so pnpm picks the same
    /// value up regardless of which config source it prefers.
    pub fn lockfile_enabled(self) -> bool {
        match self {
            BenchmarkScenario::CleanInstall => false,
            BenchmarkScenario::FrozenLockfile | BenchmarkScenario::FrozenLockfileHotCache => true,
        }
    }

    /// Whether to use a lockfile.
    pub fn lockfile<Text, LoadLockfile>(self, load_lockfile: LoadLockfile) -> Option<String>
    where
        Text: Into<String>,
        LoadLockfile: FnOnce() -> Text,
    {
        match self {
            BenchmarkScenario::CleanInstall => None,
            BenchmarkScenario::FrozenLockfile | BenchmarkScenario::FrozenLockfileHotCache => {
                load_lockfile().into().pipe(Some)
            }
        }
    }

    /// Per-iteration cleanup paths that hyperfine's `--prepare` command
    /// will `rm -rf` before each timed run (and before each warmup).
    /// Cold-cache scenarios wipe the per-revision store along with
    /// `node_modules` so every iteration starts from scratch; the
    /// hot-cache scenario only wipes `node_modules`, letting the
    /// warmup populate the store and timed iterations reuse it.
    pub fn cleanup_paths(self) -> &'static [&'static str] {
        match self {
            BenchmarkScenario::CleanInstall | BenchmarkScenario::FrozenLockfile => {
                &["node_modules", "store-dir"]
            }
            BenchmarkScenario::FrozenLockfileHotCache => &["node_modules"],
        }
    }
}

#[derive(Debug, Args)]
pub struct HyperfineOptions {
    /// Number of warmup runs to perform before the actual measured benchmark.
    #[clap(long, default_value_t = 1)]
    pub warmup: u8,

    /// Minimum number of runs for each command.
    #[clap(long)]
    pub min_runs: Option<u8>,

    /// Maximum number of runs for each command.
    #[clap(long)]
    pub max_runs: Option<u8>,

    /// Exact number of runs for each command.
    #[clap(long)]
    pub runs: Option<u8>,

    /// Print stdout and stderr of the benchmarked program instead of suppressing it
    #[clap(long)]
    show_output: bool,

    /// Ignore non-zero exit codes of the benchmarked program.
    #[clap(long)]
    pub ignore_failure: bool,
}

impl HyperfineOptions {
    pub fn append_to(&self, hyperfine_command: &mut Command) {
        let &HyperfineOptions { show_output, warmup, min_runs, max_runs, runs, ignore_failure } =
            self;
        hyperfine_command.arg("--warmup").arg(warmup.to_string());
        if let Some(min_runs) = min_runs {
            hyperfine_command.arg("--min-runs").arg(min_runs.to_string());
        }
        if let Some(max_runs) = max_runs {
            hyperfine_command.arg("--max-runs").arg(max_runs.to_string());
        }
        if let Some(runs) = runs {
            hyperfine_command.arg("--runs").arg(runs.to_string());
        }
        if show_output {
            hyperfine_command.arg("--show-output");
        }
        if ignore_failure {
            hyperfine_command.arg("--ignore-failures");
        }
    }
}
