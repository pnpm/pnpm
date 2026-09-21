use super::concurrency_group::{GroupStatus, inspect_group, render_group};
use clap::{Args, Subcommand};
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

impl TasksCommand {
    pub fn run(self, config: &Config) -> miette::Result<()> {
        match self {
            TasksCommand::Status(args) => args.run(config),
        }
    }
}

#[derive(Debug, Args)]
pub struct TasksStatusArgs {
    /// Group names to show. With none, every group that has a holder or waiter.
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
