//! Which interpreter installs a project.
//!
//! A workspace that configures `python.executable` names one for every
//! project. Otherwise each project gets the first interpreter this machine
//! has that its `requires-python` accepts, preferring the version a
//! `.python-version` file asks for.

mod command;

use super::{host, manifest::Manifest, targets};
use command::{InterpreterCommand, path_outside, scan_for_interpreters};
use host::Interpreter;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::Config;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use std::{
    ffi::OsString,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

const WRITE_TO_STRING: &str = "writing to a String cannot fail";

/// The interpreters one selection looks at, cheapest round first. The
/// scan reads every directory on the PATH, so a project that the
/// conventional names already satisfy never pays for it.
#[derive(Clone, Copy)]
enum Round {
    Named,
    Scanned,
}

/// What searching this machine's interpreters found for one project.
enum Search {
    /// One the project's range and its requested version both accept.
    Accepted(Arc<Interpreter>),
    /// One the range accepts, for a version this machine does not have.
    OtherVersion(Arc<Interpreter>),
    None,
}

/// What probing one interpreter found.
enum Probe {
    Usable(Arc<Interpreter>),
    /// Why this interpreter cannot install anything, for the report that
    /// no interpreter fits.
    Unusable(String),
}

/// The version a `.python-version` file asks for, as a prefix of an
/// interpreter's own version: `3.13` accepts every 3.13.x.
struct VersionRequest {
    release: Vec<u64>,
    file: PathBuf,
}

/// The interpreters one install has looked at, in the order it looked.
pub(super) struct Interpreters<'a> {
    config: &'a Config,
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
    pub(super) fn new(config: &'a Config) -> Self {
        Self {
            config,
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
        if let Some(executable) = &self.config.python.executable {
            let command = InterpreterCommand::program(executable);
            let interpreter = match self.probe(&command).await {
                Probe::Usable(interpreter) => Arc::clone(interpreter),
                Probe::Unusable(reason) => bail!("{reason}"),
            };
            check_requires_python(root, &interpreter, requires_python.as_ref())?;
            return Ok(interpreter);
        }
        let request = self.version_request::<Reporter>(root)?;
        match self.search(requires_python.as_ref(), request.as_ref()).await {
            Search::Accepted(interpreter) => Ok(interpreter),
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
                Ok(interpreter)
            }
            Search::None => {
                bail!("{}", self.no_interpreter(root, requires_python.as_ref(), request.as_ref()))
            }
        }
    }

    /// The first interpreter this machine has that the project accepts.
    async fn search(
        &mut self,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> Search {
        let mut other_version = None;
        for round in [Round::Named, Round::Scanned] {
            for command in self.candidates(round, request).await {
                match self.consider(&command, requires_python, request).await {
                    Search::Accepted(interpreter) => return Search::Accepted(interpreter),
                    Search::OtherVersion(interpreter) => other_version.get_or_insert(interpreter),
                    Search::None => continue,
                };
            }
        }
        other_version.map_or(Search::None, Search::OtherVersion)
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
            return Search::None;
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
                self.scanned = Some(scan_for_interpreters(&self.path).await);
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

    /// The version the nearest `.python-version` file asks for. The file
    /// belongs to other tools too, so a line pnpm cannot read is reported
    /// and ignored rather than failing the install.
    fn version_request<Reporter: self::Reporter + 'static>(
        &self,
        root: &Path,
    ) -> Result<Option<VersionRequest>> {
        let stop = self.config.workspace_dir.as_deref();
        let mut directory = Some(root);
        while let Some(current) = directory {
            let file = current.join(".python-version");
            let contents = match fs::read_to_string(&file) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("read {}", file.display()));
                }
            };
            if let Some(contents) = contents {
                let request = parse_version_request(&contents, file.clone());
                if request.is_none() {
                    Reporter::emit(&LogEvent::Global(GlobalLog {
                        level: LogLevel::Warn,
                        message: format!(
                            "Ignoring {}: pnpm reads a plain Python version such as 3.13 from it",
                            file.display(),
                        ),
                    }));
                }
                return Ok(request);
            }
            directory = (Some(current) != stop).then(|| current.parent()).flatten();
        }
        Ok(None)
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
        report.push_str(
            "\n  install an interpreter it accepts, or set python.executable in pnpm-workspace.yaml",
        );
        report
    }
}

impl VersionRequest {
    /// Whether an interpreter's version starts with the requested one,
    /// which is how `3.13` asks for every 3.13.x.
    fn accepts(&self, version: &pep440_rs::Version) -> bool {
        let release = version.release();
        self.release.len() <= release.len()
            && self.release
                .iter()
                .zip(release)
                .all(|(requested, actual)| requested == actual)
    }

    fn version(&self) -> String {
        self.release
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// The version a `.python-version` file asks for. The file may name a
/// distribution rather than a version, which is for the tool that wrote
/// it, so anything but a plain version reads as no request at all.
fn parse_version_request(contents: &str, file: PathBuf) -> Option<VersionRequest> {
    let line = contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;
    let release = line
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!release.is_empty()).then_some(VersionRequest { release, file })
}

/// A project that pins an interpreter range cannot be installed with the
/// interpreter a workspace names outside it.
fn check_requires_python(
    root: &Path,
    interpreter: &Interpreter,
    requires_python: Option<&pep440_rs::VersionSpecifiers>,
) -> Result<()> {
    let version = interpreter.target.environment.python_full_version();
    match requires_python {
        Some(specifiers) if !specifiers.contains(version) => {
            bail!("{} requires Python {specifiers}, but {version} was selected", root.display())
        }
        _ => Ok(()),
    }
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
