//! Which interpreter installs a project.
//!
//! A workspace that configures `python.executable` names one for every
//! project. Otherwise each project gets the first interpreter this machine
//! has that its `requires-python` accepts, preferring the version a
//! `.python-version` file asks for.
//!
//! `requires-python` is to an interpreter what `engines.runtime` is to a
//! Node.js runtime, so `runtimeOnFail` decides what an install with no
//! interpreter that accepts it does: install one, report the project, or
//! go on with an interpreter the machine has.

pub(crate) use mismatch::Mismatch;

mod command;
mod download;
mod mismatch;
mod request;
mod search;

use super::{host, manifest::Manifest, targets};
use command::{InterpreterCommand, path_outside, scan_for_interpreters};
use host::Interpreter;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use mismatch::check_requires_python;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use request::VersionRequest;
use search::{Fallbacks, Search};
use std::{ffi::OsString, fmt::Write as _, path::Path, sync::Arc};

const WRITE_TO_STRING: &str = "writing to a String cannot fail";

/// The interpreters one selection looks at, cheapest round first. The
/// scan reads every directory on the PATH, so a project that the
/// conventional names already satisfy never pays for it.
#[derive(Clone, Copy)]
enum Round {
    Named,
    Scanned,
}

/// What an install of an interpreter is for.
struct Install<'a> {
    requires_python: Option<&'a pep440_rs::VersionSpecifiers>,
    /// The version a `.python-version` file asks for.
    request: Option<&'a VersionRequest>,
    /// Whether a version other than the requested one may be installed,
    /// which is what a machine holding no interpreter the project accepts
    /// has to do.
    any_version: bool,
}

/// What probing one interpreter found.
enum Probe {
    Usable(Arc<Interpreter>),
    /// Why this interpreter cannot install anything, for the report that
    /// no interpreter fits.
    Unusable(String),
}

/// The interpreters one install has looked at, in the order it looked.
pub(super) struct Interpreters<'a> {
    config: &'a Config,
    client: &'a ThrottledClient,
    source: download::Source<'a>,
    probed: Vec<(InterpreterCommand, Probe)>,
    /// The interpreters the machine offers under a version, scanned for
    /// once however many projects need them.
    scanned: Option<Vec<InterpreterCommand>>,
    /// The PATH an interpreter named rather than located is resolved
    /// through: this process's own, without the workspace being
    /// installed.
    path: OsString,
}

impl<'a> Interpreters<'a> {
    pub(super) fn new(config: &'a Config, client: &'a ThrottledClient) -> Self {
        Self {
            config,
            client,
            source: download::Source::configured(config),
            probed: Vec::new(),
            scanned: None,
            path: path_outside(config.workspace_dir.as_deref()),
        }
    }

    /// The interpreter that installs the project at `root`.
    pub(super) async fn select<Reporter: self::Reporter + 'static>(
        &mut self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<Arc<Interpreter>> {
        let requires_python = requires_python(root, manifest)?;
        self.select_accepting::<Reporter>(root, requires_python.as_ref()).await
    }

    /// The interpreter that installs the projects sharing the environment
    /// at `root`: one that the range every one of them accepts contains,
    /// preferring the version a `.python-version` file at or above `root`
    /// asks for.
    pub(super) async fn select_accepting<Reporter: self::Reporter + 'static>(
        &mut self,
        root: &Path,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
    ) -> Result<Arc<Interpreter>> {
        if self.config.python.executable.is_some() {
            return self.configured::<Reporter>(root, requires_python).await;
        }
        let request = self.version_request::<Reporter>(root)?;
        let found = self.search(requires_python, request.as_ref()).await;
        if let Search::Accepted(interpreter) = found {
            return Ok(interpreter);
        }
        // A machine with no interpreter the project accepts takes any
        // version the release offers that it does accept; one that has
        // such an interpreter only installs the version asked for by
        // name, and keeps what it has otherwise.
        let install = Install {
            requires_python,
            request: request.as_ref(),
            any_version: matches!(found, Search::None | Search::Unaccepted(_)),
        };
        if let Some(interpreter) = self.install::<Reporter>(root, install).await? {
            return Ok(interpreter);
        }
        self.fall_back::<Reporter>(root, found, requires_python, request.as_ref())
    }

    /// The interpreter an install goes on with when the search found none
    /// the project accepts and none was installed: one the project asked
    /// for another version of, or one its range rejects that
    /// `runtimeOnFail` lets through.
    fn fall_back<Reporter: self::Reporter + 'static>(
        &self,
        root: &Path,
        found: Search,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> Result<Arc<Interpreter>> {
        let interpreter = match found {
            Search::Accepted(interpreter) => return Ok(interpreter),
            Search::OtherVersion(interpreter) => {
                let request = request.expect("only a version request can go unmet");
                Reporter::emit(&LogEvent::Global(GlobalLog {
                    level: LogLevel::Warn,
                    message: format!(
                        "Installing {} with Python {}: {} asks for Python {}, which was not found",
                        root.display(),
                        interpreter.target.environment.python_full_version(),
                        request.file.display(),
                        request.version(),
                    ),
                }));
                return Ok(interpreter);
            }
            Search::Unaccepted(interpreter) => interpreter,
            Search::None => bail!("{}", self.no_interpreter(root, requires_python, request)),
        };
        let accepted = mismatch::accepted::<Reporter>(
            root,
            interpreter.target.environment.python_full_version(),
            requires_python.expect("only a declared range can go unmet"),
            Mismatch::of(self.config),
        );
        if accepted {
            return Ok(interpreter);
        }
        bail!("{}", self.no_interpreter(root, requires_python, request))
    }

    /// Install an interpreter the project accepts, when this machine has
    /// none and the workspace lets pnpm install one. `None` when it does
    /// not, or when the builds pnpm can install hold nothing the project
    /// accepts.
    async fn install<Reporter: self::Reporter + 'static>(
        &mut self,
        root: &Path,
        install: Install<'_>,
    ) -> Result<Option<Arc<Interpreter>>> {
        if !download::allowed(self.config) {
            return Ok(None);
        }
        let Install {
            requires_python,
            request,
            any_version,
        } = install;
        let exact = download::exact_version(requires_python, request);
        let releases =
            download::Releases::read_from(self.config, self.client, self.source, exact.as_ref())
                .await?;
        let asked_for = releases.best(requires_python, request);
        let build = match asked_for {
            Some(build) => Some(build),
            None if any_version => releases.best(requires_python, None),
            None => None,
        };
        let Some(build) = build else { return Ok(None) };
        if let Some(request) = request.filter(|_| asked_for.is_none()) {
            download::report_unmet_request::<Reporter>(&releases, root, request, build.version());
        }
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Info,
            message: format!("Installing Python {} for {}", build.version(), root.display()),
        }));
        let command = build.install(self.config, self.client).await?;
        match self.probe(&command).await {
            Probe::Usable(interpreter) => Ok(Some(Arc::clone(interpreter))),
            Probe::Unusable(reason) => bail!("{reason}"),
        }
    }

    /// The interpreter the workspace names, which is the only one a
    /// project of that workspace is installed with.
    async fn configured<Reporter: self::Reporter + 'static>(
        &mut self,
        root: &Path,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
    ) -> Result<Arc<Interpreter>> {
        let executable =
            self.config.python.executable.as_ref().expect("the workspace names an interpreter");
        let command = InterpreterCommand::program(executable);
        let interpreter = match self.probe(&command).await {
            Probe::Usable(interpreter) => Arc::clone(interpreter),
            Probe::Unusable(reason) => bail!("{reason}"),
        };
        check_requires_python::<Reporter>(
            root,
            &interpreter,
            requires_python,
            Mismatch::of(self.config),
        )?;
        Ok(interpreter)
    }

    /// The first interpreter this machine has that the project accepts.
    async fn search(
        &mut self,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> Search {
        let mut fallbacks = Fallbacks::default();
        for round in [Round::Named, Round::Scanned] {
            for command in self.candidates(round, request).await {
                let found = self.consider(&command, requires_python, request).await;
                if let Search::Accepted(interpreter) = found {
                    return Search::Accepted(interpreter);
                }
                fallbacks.keep(found, request);
            }
        }
        fallbacks.best()
    }

    /// What one interpreter is to the project: the one to install it, one
    /// of a version other than the requested, or nothing.
    async fn consider(
        &mut self,
        command: &InterpreterCommand,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> Search {
        let Probe::Usable(interpreter) = self.probe(command).await else { return Search::None };
        let interpreter = Arc::clone(interpreter);
        let version = interpreter.target.environment.python_full_version();
        if requires_python.is_some_and(|specifiers| !specifiers.contains(version)) {
            return Search::Unaccepted(interpreter);
        }
        if request.is_some_and(|request| !request.accepts(version)) {
            return Search::OtherVersion(interpreter);
        }
        Search::Accepted(interpreter)
    }

    /// The interpreters of one round, in the order that reaches a usable
    /// one with the fewest interpreter starts.
    async fn candidates(
        &mut self,
        round: Round,
        request: Option<&VersionRequest>,
    ) -> Vec<InterpreterCommand> {
        if matches!(round, Round::Scanned) {
            if self.scanned.is_none() {
                let installed = self.config.store_dir.root().join("python");
                self.scanned = Some(scan_for_interpreters(&self.path, &installed).await);
            }
            return self.scanned.clone().expect("the machine was just scanned");
        }
        Self::named(request)
            .into_iter()
            .filter_map(|command| command.located(&self.path))
            .collect()
    }

    /// The names an interpreter is conventionally installed under, and
    /// the one the requested version is installed under.
    fn named(request: Option<&VersionRequest>) -> Vec<InterpreterCommand> {
        let mut names = if cfg!(windows) {
            vec![InterpreterCommand::program("python"), InterpreterCommand::program("python3")]
        } else {
            vec![InterpreterCommand::program("python3"), InterpreterCommand::program("python")]
        };
        if let Some([major, minor]) = request
            .map(|request| request.release.as_slice())
            .and_then(|release| <[u64; 2]>::try_from(&release[..release.len().min(2)]).ok())
        {
            names.push(InterpreterCommand::program(format!("python{major}.{minor}")));
            if cfg!(windows) {
                names.push(InterpreterCommand::launcher(format!("-{major}.{minor}")));
            }
        }
        names
    }

    /// Probe an interpreter, at most once per install. It reports what it
    /// is, and what every environment the project locks for resolves as.
    async fn probe(&mut self, command: &InterpreterCommand) -> &Probe {
        let mut index = self.probed
            .iter()
            .position(|(probed, _)| probed == command);
        if index.is_none() {
            let program = host::Program {
                executable: &command.program,
                arguments: &command.arguments,
                path: Some(&self.path),
            };
            let probe = match host::run_program::<Interpreter>(
                program,
                "probe",
                targets::probe_request(self.config),
            )
            .await
            {
                Ok(interpreter) => Probe::Usable(Arc::new(interpreter)),
                Err(error) => Probe::Unusable(format!("{command}: {error}")),
            };
            self.probed.push((command.clone(), probe));
            index = Some(self.probed.len() - 1);
        }
        &self.probed[index.expect("the interpreter was just probed")].1
    }

    /// The version the nearest `.python-version` file asks for.
    fn version_request<Reporter: self::Reporter + 'static>(
        &self,
        root: &Path,
    ) -> Result<Option<VersionRequest>> {
        request::version_request::<Reporter>(self.config.workspace_dir.as_deref(), root)
    }

    /// What was asked for and what this machine has, for the install that
    /// cannot go on without an interpreter.
    ///
    /// When no interpreter ran at all, the reason the first one did not is
    /// the whole story: a request every interpreter rejects, such as an
    /// environment the project declares that pnpm cannot resolve for, says
    /// more than a list of interpreters that all failed the same way.
    fn no_interpreter(
        &self,
        root: &Path,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> String {
        if let Some((_, Probe::Unusable(reason))) = self.probed.first()
            && self.probed
                .iter()
                .all(|(_, probe)| matches!(probe, Probe::Unusable(_)))
        {
            return reason.clone();
        }
        let mut report = format!("no Python interpreter for {}", root.display());
        if let Some(specifiers) = requires_python {
            write!(report, "\n  the project requires Python {specifiers}").expect(WRITE_TO_STRING);
        }
        if let Some(request) = request {
            write!(report, "\n  {} asks for Python {}", request.file.display(), request.version())
                .expect(WRITE_TO_STRING);
        }
        for (command, probe) in &self.probed {
            match probe {
                Probe::Usable(interpreter) => write!(
                    report,
                    "\n  {command} is Python {}",
                    interpreter.target.environment.python_full_version(),
                ),
                Probe::Unusable(_) => write!(report, "\n  {command} did not run"),
            }
            .expect(WRITE_TO_STRING);
        }
        if let Some(reason) = download::refused(self.config) {
            write!(report, "\n  pnpm installed none because {reason}").expect(WRITE_TO_STRING);
        }
        report.push_str(advice(
            self.probed
                .iter()
                .any(|(_, probe)| matches!(probe, Probe::Usable(_))),
        ));
        report
    }
}

/// What the report tells the reader to do. An install that ran an
/// interpreter can also go on with the one it ran, which one that ran
/// none cannot.
fn advice(ran_one: bool) -> &'static str {
    if ran_one {
        return "\n  install an interpreter it accepts, set python.executable in pnpm-workspace.yaml, or set runtimeOnFail to warn or ignore to install with one it rejects";
    }
    "\n  install an interpreter it accepts, or set python.executable in pnpm-workspace.yaml"
}

/// The interpreter range every one of `members` declares, as one range:
/// the versions all of them accept. `None` when none declares one.
pub(super) fn requires_python_of<'a>(
    members: impl IntoIterator<Item = (&'a Path, &'a Manifest)>,
) -> Result<Option<pep440_rs::VersionSpecifiers>> {
    let mut clauses = Vec::<pep440_rs::VersionSpecifier>::new();
    let mut declared = false;
    for (root, manifest) in members {
        let Some(specifiers) = requires_python(root, manifest)? else { continue };
        declared = true;
        for specifier in specifiers.iter() {
            if !clauses.contains(specifier) {
                clauses.push(specifier.clone());
            }
        }
    }
    Ok(declared.then(|| clauses.into_iter().collect()))
}

/// The interpreter range the project declares.
fn requires_python(
    root: &Path,
    manifest: &Manifest,
) -> Result<Option<pep440_rs::VersionSpecifiers>> {
    manifest.project
        .as_ref()
        .and_then(|project| project.requires_python.as_deref())
        .map(str::parse)
        .transpose()
        .into_diagnostic()
        .wrap_err_with(|| format!("read requires-python of {}", root.display()))
}

#[cfg(test)]
mod tests;
