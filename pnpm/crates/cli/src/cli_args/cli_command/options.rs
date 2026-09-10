use super::{
    CliArgs, CliCommand, CommandFactory, Diagnostic, Display, Error, ErrorKind, LogLevelSetting,
    Pipe, ReporterType,
};

/// Error type of [`CliArgs::apply_workspace_root`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WorkspaceRootError {
    #[display("--workspace-root may not be used with --global")]
    #[diagnostic(code(ERR_PNPM_OPTIONS_CONFLICT))]
    GlobalConflict,

    #[display("--workspace-root may only be used inside a workspace")]
    #[diagnostic(code(ERR_PNPM_NOT_IN_WORKSPACE))]
    NotInWorkspace,

    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),
}

impl CliArgs {
    /// The reporter the command should drive: `--loglevel silent` forces
    /// the silent reporter over any `--reporter` choice, mirroring the
    /// reporter selection in pnpm 11's `main.ts`.
    pub(crate) fn effective_reporter(&self) -> ReporterType {
        if self.loglevel == Some(LogLevelSetting::Silent) {
            return ReporterType::Silent;
        }
        self.reporter
    }

    pub fn validate_command_scoped_global_options(&self) -> Result<(), clap::Error> {
        if self.resume_from.is_some() {
            self.validate_run_scoped_global_option("--resume-from")?;
        }
        if self.report_summary {
            self.validate_report_summary_global_option()?;
        }
        if self.no_bail {
            self.validate_no_bail_global_option()?;
        }
        if self.if_present {
            self.validate_if_present_top_level_option()?;
        }
        if self.parallel {
            self.validate_parallel_global_option()?;
        }
        if self.reporter_hide_prefix {
            self.validate_run_scoped_global_option("--reporter-hide-prefix")?;
        }
        if self.no_reporter_hide_prefix {
            self.validate_run_scoped_global_option("--no-reporter-hide-prefix")?;
        }
        Ok(())
    }

    /// Promote the command to recursive mode when a `--filter` /
    /// `--filter-prod` selector is present, even without an explicit
    /// `-r` / `--recursive`.
    ///
    /// Setting `recursive = true` whenever a filter is given applies
    /// CLI-wide rather than being special-cased per command. Call once on
    /// the parsed args before dispatch; both the install fast-path bail
    /// and [`Self::run`] then observe the promoted flag.
    pub fn promote_recursive_for_filter(&mut self) {
        if !self.filter.is_empty() || !self.filter_prod.is_empty() {
            self.recursive = true;
        }
    }

    /// Apply the recursive-run settings represented by pnpm's
    /// `--parallel` shorthand.
    pub fn apply_parallel_run_options(&mut self) {
        if self.parallel {
            self.recursive = true;
            self.no_sort = true;
            self.stream = true;
        }
    }

    /// Resolve a `--dir` the command line did not give to pnpm's local
    /// prefix: the nearest ancestor of the process cwd that holds a
    /// project. pnpm's `config.dir` is that prefix, so a command run from
    /// a plain subdirectory of a project acts on the project rather than
    /// on the subdirectory. Which manifests count as a project depends on
    /// the command — see [`CliCommand::acts_on_the_npm_project`]. Call
    /// before anything reads `--dir`.
    ///
    /// A cwd that cannot be read leaves `--dir` at its default, which the
    /// canonicalization in [`Self::run`] then reports on.
    pub fn apply_local_prefix(&mut self) -> miette::Result<()> {
        if self.dir_from_command_line {
            return Ok(());
        }
        let Ok(cwd) = std::env::current_dir() else {
            return Ok(());
        };
        self.dir = if self.command.acts_on_the_npm_project() {
            super::super::prefix::find_npm_local_prefix(&cwd)?
        } else {
            super::super::prefix::find_local_prefix(&cwd)?
        };
        Ok(())
    }

    /// Apply `--workspace-root` / `-w`: point `--dir` at the workspace
    /// root so the command runs on the root project. Call after
    /// [`Self::promote_recursive_for_filter`] and before anything reads
    /// `--dir`, matching where pnpm's CLI parser applies it.
    ///
    /// The `--dir` resolution mirrors pnpm's `findWorkspaceDir`: real path
    /// first, because a case-insensitive filesystem otherwise finds the
    /// root under one spelling and the members under another; then a
    /// lexical fallback, so an unresolvable `--dir` is not fatal.
    ///
    /// The fallback resolves `..` itself rather than leaving the components
    /// in place. [`pnpm_workspace::find_workspace_dir`] walks ancestors
    /// lexically, so a `--dir` that climbs out of the workspace and lands
    /// on a directory that does not exist — `../../elsewhere` — would
    /// otherwise walk right back up through its own `..` components and
    /// select the workspace the user pointed away from.
    pub fn apply_workspace_root(&mut self) -> Result<(), WorkspaceRootError> {
        if !self.workspace_root {
            return Ok(());
        }
        if self.command.is_global() {
            return Err(WorkspaceRootError::GlobalConflict);
        }
        let dir = dunce::canonicalize(&self.dir)
            .or_else(|_| std::path::absolute(&self.dir))
            .unwrap_or_else(|_| self.dir.clone())
            .pipe_deref(pnpm_fs::lexical_normalize);
        let workspace_dir = pnpm_workspace::find_workspace_dir(&dir)
            .map_err(WorkspaceRootError::FindWorkspaceDir)?
            .ok_or(WorkspaceRootError::NotInWorkspace)?;
        self.dir = workspace_dir;
        // pnpm's parser writes the workspace root into `cliOptions.dir`, so
        // `-w` also decides where `init` scaffolds.
        self.dir_from_command_line = true;
        Ok(())
    }

    /// Promote commands marked recursive-by-default by pnpm when they run
    /// inside a workspace.
    pub fn promote_recursive_by_default(&mut self) {
        let dir = dunce::canonicalize(&self.dir)
            .or_else(|_| std::path::absolute(&self.dir))
            .unwrap_or_else(|_| self.dir.clone())
            .pipe_deref(pnpm_fs::lexical_normalize);
        if !self.recursive
            && self.command.recursive_by_default()
            && pnpm_workspace::find_workspace_dir(&dir).is_ok_and(|dir| dir.is_some())
        {
            self.recursive = true;
        }
    }

    /// `restart` also runs scripts and accepts its own `--if-present`,
    /// so the top-level spelling is valid for it too — unlike the
    /// recursive-only flags, which `restart` rejects. `exec` is the
    /// reverse: it takes the recursive-only flags but runs arbitrary
    /// commands rather than scripts, so pnpm rejects `--if-present`
    /// for it and pacquet must too.
    fn validate_if_present_top_level_option(&self) -> Result<(), clap::Error> {
        match self.command {
            CliCommand::Restart(_) => Ok(()),
            CliCommand::Exec(_) => Err(Self::unexpected_argument_error("--if-present")),
            _ => self.validate_run_scoped_global_option("--if-present"),
        }
    }

    fn validate_run_scoped_global_option(&self, option: &str) -> Result<(), clap::Error> {
        if matches!(
            self.command,
            CliCommand::Run(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Test(_)
                | CliCommand::Start(_)
                | CliCommand::Stop(_),
        ) {
            return Ok(());
        }
        Err(Self::unexpected_argument_error(option))
    }

    fn unexpected_argument_error(option: &str) -> clap::Error {
        Self::command()
            .error(ErrorKind::UnknownArgument, format!("unexpected argument '{option}' found"))
    }

    fn validate_report_summary_global_option(&self) -> Result<(), clap::Error> {
        if matches!(self.command, CliCommand::Publish(_) | CliCommand::Stage(_)) {
            return Ok(());
        }
        self.validate_run_scoped_global_option("--report-summary")
    }

    fn validate_no_bail_global_option(&self) -> Result<(), clap::Error> {
        if matches!(self.command, CliCommand::Rebuild(_)) {
            return Ok(());
        }
        self.validate_run_scoped_global_option("--no-bail")
    }

    fn validate_parallel_global_option(&self) -> Result<(), clap::Error> {
        if matches!(
            self.command,
            CliCommand::Run(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Test(_)
                | CliCommand::Start(_)
                | CliCommand::Stop(_),
        ) {
            return Ok(());
        }
        Err(Self::unexpected_argument_error("--parallel"))
    }
}
