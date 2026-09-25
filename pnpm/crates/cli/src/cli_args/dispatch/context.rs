use super::configuration::{RunAnchors, apply_update_config};
use crate::{
    State,
    cli_args::reporter::{ReporterFlags, ReporterType},
};
use miette::Context;
use pnpm_config::Config;
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

pub(crate) type CommandFuture<'a, Output = ()> =
    Pin<Box<dyn Future<Output = miette::Result<Output>> + Send + 'a>>;

/// The shared context every subcommand handler needs: the canonicalized
/// `--dir`, the derived `package.json` path, the selected reporter, the
/// `--recursive` flag, and the lazily-loaded config loaders the handlers
/// pull from on demand.
///
/// The loaders are passed as `&dyn Fn` rather than eagerly loaded so a
/// handler that never needs them (`pacquet init`) doesn't pay for the
/// `.npmrc` read, and so each call re-loads a fresh `&'static mut Config`.
/// The closures are built in [`CliArgs::run`](crate::cli_args::cli_command::CliArgs::run).
pub(crate) struct RunCtx<'a> {
    pub(crate) effective_reporter: &'a AtomicU8,
    pub(crate) reporter_flags: ReporterFlags,
    /// Whether a `pm` prefix (`pnpm pm clean`) forced the built-in
    /// command, so a `package.json` script of the same name must not
    /// override it. See [`crate::pm_prefix`].
    pub(crate) builtin_command_forced: bool,
    /// Set by [`crate::cli_args::script_override::resolve`] when a same-named
    /// `package.json` script replaces the built-in command. The run then
    /// prints what `pnpm run` prints, so the install-family `Done in ...`
    /// footer of the command that was typed has to stay out of it.
    pub(crate) builtin_replaced_by_script: &'a AtomicBool,
    pub(crate) locations: CommandLocations<'a>,
    pub(crate) workspace: WorkspaceInvocation<'a>,
    pub(crate) loaders: CommandLoaders<'a>,
}

pub(crate) struct CommandLocations<'a> {
    pub(crate) dir: &'a Path,
    /// The `--dir` as the command line gave it, or the process cwd when it
    /// gave none — pnpm's `cliOptions.dir ?? process.cwd()`. `init`
    /// scaffolds here and a non-recursive `exec` runs here, rather than at
    /// the local prefix [`Self::dir`] resolves to.
    pub(crate) cli_dir: &'a Path,
    pub(crate) manifest_path: &'a Path,
}

pub(crate) struct WorkspaceInvocation<'a> {
    pub(crate) recursive: bool,
    pub(crate) resume_from: Option<&'a str>,
    pub(crate) report_summary: bool,
    pub(crate) parallel: bool,
    /// The top-level `--if-present` spelling (`pnpm --if-present test`);
    /// merged with the flag the script subcommands declare themselves.
    pub(crate) if_present: bool,
}

pub(crate) struct CommandLoaders<'a> {
    pub(crate) config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    /// Like [`Self::config`] but anchored at the pnpm home dir instead of
    /// `--dir`, so a `-g` install can't inherit the caller project's
    /// `.npmrc` network / TLS / registry settings.
    pub(crate) global_config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    /// Like [`Self::config`] but loaded through
    /// [`Config::current_for_self_update`], so a repo-controlled
    /// `pnpm-workspace.yaml` can only tighten the release-age policy that
    /// governs the pnpm download.
    pub(crate) config_self_update: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
}

impl<'a> RunCtx<'a> {
    pub(crate) fn reporter(&self) -> ReporterType {
        self.effective_reporter.load(Ordering::Relaxed).into()
    }

    /// The command's [`Config`] with the `updateConfig` hooks applied, as a
    /// future a handler can move into the [`CommandFuture`] it dispatches.
    /// The hooks run once per call, so a handler that needs more than one
    /// [`State`] builds each from the one config this yields.
    pub(in crate::cli_args) fn prepared_config(
        &self,
    ) -> impl Future<Output = miette::Result<&'static Config>> + Send + 'a {
        self.prepared_config_with(|_| {})
    }

    /// [`Self::prepared_config`] with the command's own settings applied
    /// first, for a command whose flags decide what the hook pass does,
    /// such as `--ignore-pnpmfile`.
    pub(in crate::cli_args) fn prepared_config_with(
        &self,
        apply_cli_config: impl FnOnce(&mut Config) + Send + 'a,
    ) -> impl Future<Output = miette::Result<&'static Config>> + Send + 'a {
        let load_config = self.loaders.config;
        let dir = self.locations.dir;
        let effective_reporter = self.effective_reporter;
        async move {
            let config = load_config()?;
            apply_cli_config(config);
            let reporter = effective_reporter.load(Ordering::Relaxed).into();
            apply_update_config(config, dir, reporter).await?;
            Ok(&*config)
        }
    }

    /// The command's [`State`], built on [`Self::prepared_config`].
    /// `require_lockfile` is [`State::init`]'s.
    pub(in crate::cli_args) fn prepared_state(
        &self,
        require_lockfile: bool,
    ) -> impl Future<Output = miette::Result<State>> + Send + 'a {
        self.prepared_state_with(require_lockfile, |_| {})
    }

    /// [`Self::prepared_state`] on [`Self::prepared_config_with`].
    pub(in crate::cli_args) fn prepared_state_with(
        &self,
        require_lockfile: bool,
        apply_cli_config: impl FnOnce(&mut Config) + Send + 'a,
    ) -> impl Future<Output = miette::Result<State>> + Send + 'a {
        let config = self.prepared_config_with(apply_cli_config);
        let manifest_path = self.locations.manifest_path;
        async move {
            State::init(manifest_path.to_path_buf(), config.await?, require_lockfile)
                .wrap_err("initialize the state")
        }
    }
}

impl<'a> From<&'a RunAnchors> for CommandLocations<'a> {
    fn from(anchors: &'a RunAnchors) -> Self {
        Self { dir: &anchors.dir, cli_dir: &anchors.cli_dir, manifest_path: &anchors.manifest_path }
    }
}

impl<'a> From<&'a crate::cli_args::cli_command::CliWorkspaceArgs> for WorkspaceInvocation<'a> {
    fn from(args: &'a crate::cli_args::cli_command::CliWorkspaceArgs) -> Self {
        Self {
            recursive: args.recursive,
            resume_from: args.ordering.resume_from.as_deref(),
            report_summary: args.execution.report_summary,
            parallel: args.ordering.parallel,
            if_present: args.execution.if_present,
        }
    }
}
