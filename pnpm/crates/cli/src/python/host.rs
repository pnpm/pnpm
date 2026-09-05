use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::MarkerEnvironment;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Interpreter {
    pub(super) executable: String,
    pub(super) environment: MarkerEnvironment,
    pub(super) tags: Vec<String>,
}

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

#[derive(Debug, Clone, Serialize)]
pub(super) struct Wheel {
    pub(super) files: BTreeMap<String, PathBuf>,
    pub(super) metadata: WheelMetadata,
}

pub(super) async fn run<Output: DeserializeOwned>(
    executable: &str,
    operation: &str,
    input: serde_json::Value,
) -> Result<Output> {
    let mut child = Command::new(executable)
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
