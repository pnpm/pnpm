use crate::commands;
use clap::{
    Parser,
    Subcommand,
};
use miette::IntoDiagnostic as _;
use std::{
    ffi::OsString,
    future::Future,
    path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(name = "pnpz", version, about = "System packages from the Nix binary cache, without Nix")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add packages to `pnpz.yaml` and install them.
    Add {
        /// Packages as `<name>` or `<name>@<version>`, for example `python@3.12`.
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Install the packages `pnpz.yaml` lists.
    Install {
        /// Fail if `pnpz-lock.yaml` does not match `pnpz.yaml`.
        #[arg(long)]
        frozen: bool,
    },
    /// Run a command with the project's packages on `PATH`.
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    /// Start a shell with the project's packages on `PATH`.
    Shell,
    /// Mounts a store at `/nix` and runs a command. `run` and `shell` call
    /// this in a fresh process, because namespaces need a single thread.
    #[command(name = "__enter", hide = true)]
    Enter {
        store_root: PathBuf,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

/// Parses the command line and runs the command it names.
pub fn run() -> miette::Result<()> {
    match Args::parse().command {
        Command::Enter { store_root, command } => commands::enter(&store_root, &command),
        Command::Add { packages } => block_on(commands::add(&packages)),
        Command::Install { frozen } => block_on(commands::install(frozen)),
        Command::Run { command } => commands::run(&command),
        Command::Shell => commands::shell(),
    }
}

pub(crate) fn block_on<Output>(
    future: impl Future<Output = miette::Result<Output>>,
) -> miette::Result<Output> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(future)
}
