//! The version a `.python-version` file asks for.

use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pnpm_reporter::{
    GlobalLog,
    LogEvent,
    LogLevel,
    Reporter,
};
use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

/// The version a `.python-version` file asks for, as a prefix of an
/// interpreter's own version: `3.13` accepts every 3.13.x.
pub(super) struct VersionRequest {
    pub(super) release: Vec<u64>,
    pub(super) file: PathBuf,
}

impl VersionRequest {
    /// A request for the release this names, for a test that has no
    /// `.python-version` file to read one from.
    #[cfg(test)]
    pub(super) fn asking_for(release: &[u64]) -> Self {
        Self { release: release.to_vec(), file: PathBuf::from(".python-version") }
    }

    /// Whether an interpreter's version starts with the requested one,
    /// which is how `3.13` asks for every 3.13.x.
    pub(super) fn accepts(&self, version: &pep440_rs::Version) -> bool {
        let release = version.release();
        self.release.len() <= release.len()
            && self.release
                .iter()
                .zip(release)
                .all(|(requested, actual)| requested == actual)
    }

    pub(super) fn version(&self) -> String {
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
pub(super) fn parse_version_request(contents: &str, file: PathBuf) -> Option<VersionRequest> {
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

/// The version the nearest `.python-version` file asks for, searched
/// from the project up to the workspace. The file belongs to other tools
/// too, so a line pnpm cannot read is reported and ignored rather than
/// failing the install.
pub(super) fn version_request<Reporter: self::Reporter + 'static>(
    stop: Option<&Path>,
    root: &Path,
) -> Result<Option<VersionRequest>> {
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
