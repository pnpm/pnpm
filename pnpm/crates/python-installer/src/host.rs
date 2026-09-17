use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_python_resolver::Target;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Interpreter {
    pub(super) executable: String,
    /// What this interpreter resolves as: its marker environment and the
    /// wheel tags it accepts, in preference order.
    #[serde(flatten)]
    pub(super) target: Target,
    /// What the environments the project declares resolve as, in the
    /// order they were asked for. Empty when it declares none.
    #[serde(default)]
    pub(super) targets: Vec<Target>,
}

/// Everything the interpreter reports about a wheel: what resolution
/// reads ([`pnpm_python_resolver::WheelMetadata`]) plus what installing it
/// needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WheelMetadata {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) requires_dist: Vec<String>,
    pub(super) requires_python: Option<String>,
    pub(super) provides_extra: Vec<String>,
    pub(super) dist_info: String,
    pub(super) purelib: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Wheel {
    /// The name of the wheel file this was unpacked from, which says
    /// which build of the version it is.
    #[serde(skip)]
    pub(super) filename: String,
    pub(super) files: BTreeMap<String, PathBuf>,
    pub(super) metadata: WheelMetadata,
    /// Where a wheel came from, which PEP 610 has
    /// the installer record rather than the backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) direct_url: Option<DirectUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DirectUrl {
    url: String,
    #[serde(flatten)]
    origin: DirectUrlOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DirectUrlOrigin {
    Directory {
        dir_info: DirectoryInfo,
    },
    Archive {
        archive_info: ArchiveInfo,
    },
    Vcs {
        vcs_info: VcsInfo,
        #[serde(skip_serializing_if = "Option::is_none")]
        subdirectory: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DirectoryInfo {
    editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveInfo {
    hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VcsInfo {
    vcs: String,
    requested_revision: String,
    commit_id: String,
}

impl DirectUrl {
    pub(super) fn directory(url: String, editable: bool) -> Self {
        Self { url, origin: DirectUrlOrigin::Directory { dir_info: DirectoryInfo { editable } } }
    }

    pub(super) fn git(source: &pnpm_python_resolver::LockedVcs) -> Self {
        Self {
            url: source.url.clone(),
            origin: DirectUrlOrigin::Vcs {
                vcs_info: VcsInfo {
                    vcs: "git".to_string(),
                    requested_revision: source.requested_revision.clone(),
                    commit_id: source.commit_id.clone(),
                },
                subdirectory: source.subdirectory.clone(),
            },
        }
    }

    pub(super) fn archive(wheel: &pnpm_python_resolver::LockedWheel) -> Self {
        Self {
            url: wheel.url.clone(),
            origin: DirectUrlOrigin::Archive {
                archive_info: ArchiveInfo { hashes: wheel.hashes.clone() },
            },
        }
    }
}

pub(super) async fn inspect(
    executable: &str,
    files: &BTreeMap<String, PathBuf>,
) -> Result<WheelMetadata> {
    run(executable, "inspect", serde_json::json!({"files": files})).await
}

/// How to start one interpreter: the program, the arguments of its own
/// that come before the helper's, such as the Windows launcher's
/// `-3.13`, and the PATH it resolves a bare program name through.
pub(super) struct Program<'a> {
    pub(super) executable: &'a str,
    pub(super) arguments: &'a [String],
    /// `None` leaves the child the PATH of this process.
    pub(super) path: Option<&'a OsStr>,
}

impl<'a> Program<'a> {
    pub(super) fn new(executable: &'a str) -> Self {
        Self { executable, arguments: &[], path: None }
    }
}

pub(super) async fn run<Output: DeserializeOwned>(
    executable: &str,
    operation: &str,
    input: serde_json::Value,
) -> Result<Output> {
    run_program(Program::new(executable), operation, input).await
}

pub(super) async fn run_program<Output: DeserializeOwned>(
    program: Program<'_>,
    operation: &str,
    input: serde_json::Value,
) -> Result<Output> {
    let Program { executable, arguments, path } = program;
    let mut command = Command::new(executable);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command
        .args(arguments)
        .args(["-I", "-c", include_str!("host.py"), operation])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .into_diagnostic()
        .wrap_err_with(|| format!("start Python interpreter {executable}"))?;
    let mut stdin = child.stdin.take().expect("child stdin was piped");
    let input = serde_json::to_vec(&input).into_diagnostic()?;
    let write = async move {
        stdin.write_all(&input).await.into_diagnostic()?;
        drop(stdin);
        Ok::<_, miette::Report>(())
    };
    let output = child.wait_with_output();
    let (written, output) = tokio::join!(write, output);
    let output = output.into_diagnostic()?;
    if !output.status.success() {
        bail!("Python {operation} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    written?;
    serde_json::from_slice(&output.stdout).into_diagnostic()
}

pub(super) async fn install(
    executable: &str,
    root: &std::path::Path,
    packages: impl Serialize,
    mode: pnpm_config::PythonLinkMode,
) -> Result<()> {
    let imports: Installation = run(
        executable,
        "install",
        serde_json::json!({"root": root, "packages": packages, "defer_files": mode != pnpm_config::PythonLinkMode::Copy}),
    )
    .await?;
    tokio::task::spawn_blocking(move || import_files(imports.imports, mode))
        .await
        .into_diagnostic()
        .wrap_err("join Python wheel imports")?
}

fn import_files(files: Vec<FileImport>, mode: pnpm_config::PythonLinkMode) -> Result<()> {
    let mut copy_devices = std::collections::BTreeSet::new();
    for file in files {
        let method = if copy_devices.contains(&file.device) {
            pnpm_config::PythonLinkMode::Copy
        } else {
            mode
        };
        let copied = file
            .import::<pnpm_fs::Host>(method)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "import Python wheel file {} to {} ({mode:?})",
                    file.source.display(),
                    file.destination.display(),
                )
            })?;
        if copied {
            copy_devices.insert(file.device);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Installation {
    imports: Vec<FileImport>,
}

#[derive(Deserialize)]
struct FileImport {
    source: PathBuf,
    destination: PathBuf,
    executable: bool,
    device: u64,
}

impl FileImport {
    fn import<Sys: pnpm_fs::FsReflink>(
        &self,
        mode: pnpm_config::PythonLinkMode,
    ) -> std::io::Result<bool> {
        use pnpm_config::PythonLinkMode;
        use std::fs;
        let copied = match mode {
            PythonLinkMode::Copy => fs::copy(&self.source, &self.destination).map(|_| true)?,
            PythonLinkMode::Hardlink => match fs::hard_link(&self.source, &self.destination) {
                Ok(()) => false,
                Err(error) if pnpm_fs::is_cross_device(&error) => {
                    fs::copy(&self.source, &self.destination)?;
                    true
                }
                Err(error) => return Err(error),
            },
            PythonLinkMode::Reflink => match Sys::reflink(&self.source, &self.destination) {
                Ok(()) => false,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::AlreadyExists,
                    ) =>
                {
                    return Err(error);
                }
                Err(_) => {
                    fs::copy(&self.source, &self.destination)?;
                    true
                }
            },
        };
        if self.executable && (mode != PythonLinkMode::Hardlink || copied) {
            pnpm_fs::file_mode::set_path_permissions(&self.destination, 0o755)?;
        }
        Ok(copied)
    }
}

#[cfg(test)]
mod tests;
