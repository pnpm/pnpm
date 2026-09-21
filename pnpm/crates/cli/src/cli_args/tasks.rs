use super::concurrency_group::{GroupStatus, inspect_group, render_group};
use clap::{Args, Subcommand, error::ErrorKind as ClapErrorKind};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Subcommand)]
pub enum TasksCommand {
    /// Show running and waiting tasks in concurrency groups.
    Status(TasksStatusArgs),
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_precedence_over_arg = true)]
pub struct TasksArgs {
    #[command(subcommand)]
    pub command: Option<TasksCommand>,

    /// Arguments passed to an overriding tasks script.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub args: Vec<String>,
}

impl TasksArgs {
    pub(crate) fn script_args(&self) -> Vec<String> {
        match &self.command {
            Some(TasksCommand::Status(args)) => std::iter::once("status".to_string())
                .chain(args.groups.iter().cloned())
                .collect(),
            None => self.args.clone(),
        }
    }

    pub fn run(self, config: &Config) -> miette::Result<()> {
        match self.command {
            Some(TasksCommand::Status(args)) => args.run(config),
            None => {
                let (kind, message) = match self.args.first() {
                    Some(name) => (
                        ClapErrorKind::InvalidSubcommand,
                        format!("unrecognized tasks subcommand {name:?}"),
                    ),
                    None => (
                        ClapErrorKind::MissingSubcommand,
                        "a tasks subcommand is required".to_string(),
                    ),
                };
                Err(Self::augment_args(clap::Command::new("pnpm tasks")).error(kind, message))
                    .into_diagnostic()
            }
        }
    }
}

#[derive(Debug, Args)]
pub struct TasksStatusArgs {
    /// Group names to show. With none, every group that has a holder or waiter.
    #[arg(allow_hyphen_values = true)]
    pub groups: Vec<String>,
}

impl TasksStatusArgs {
    pub fn run(self, config: &Config) -> miette::Result<()> {
        let slots_root = config.state_dir.join("run-slots");
        let named = !self.groups.is_empty();
        let names = if named {
            self.groups
        } else {
            group_names(&slots_root)
                .into_diagnostic()
                .wrap_err("failed to list concurrency groups")?
        };
        let mut rendered = Vec::new();
        for name in names {
            let status = inspect_named(&slots_root, &name)?;
            if named || !status.is_idle() {
                rendered.push(render_group(&name, &status));
            }
        }
        if rendered.is_empty() {
            println!("No concurrency groups are in use.");
        } else {
            println!("{}", rendered.join("\n\n"));
        }
        Ok(())
    }
}

fn inspect_named(slots_root: &Path, name: &str) -> miette::Result<GroupStatus> {
    let dir = group_dir(slots_root, name)?;
    inspect_group(&dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to inspect concurrency group {name}"))
}

fn group_names(slots_root: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(slots_root) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        other => other?,
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry
            .file_name()
            .to_str()
            .map(str::to_string)
        else {
            continue;
        };
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn group_dir(slots_root: &Path, name: &str) -> miette::Result<PathBuf> {
    let mut components = Path::new(name).components();
    let plain =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !plain {
        return Err(miette::miette!("invalid concurrency group name {name:?}"));
    }
    Ok(slots_root.join(name))
}

#[cfg(test)]
mod tests;
