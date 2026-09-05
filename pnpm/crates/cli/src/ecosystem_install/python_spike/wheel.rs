use super::{LockedPackage, exact_requirement};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use miette::{IntoDiagnostic, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(super) async fn validate(
    package: &LockedPackage,
    files: &BTreeMap<String, PathBuf>,
) -> Result<Vec<String>> {
    let dist_info = format!("{}-{}.dist-info", package.name.replace('-', "_"), package.version);
    for path in files.keys() {
        if !path.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-._/".contains(&byte))
            || path.split('/').any(|part| part.ends_with(".data"))
            || path.ends_with("/entry_points.txt")
            || path.split('/').any(|part| part.ends_with(".dist-info") && part != dist_info)
        {
            bail!("unsupported Python spike wheel path: {path}");
        }
    }
    let wheel = read(files, &format!("{dist_info}/WHEEL")).await?;
    let wheel = headers(&wheel)?;
    if scalar(&wheel, "Wheel-Version")? != "1.0"
        || scalar(&wheel, "Root-Is-Purelib")? != "true"
        || scalar(&wheel, "Tag")? != "py3-none-any"
    {
        bail!("Python spike requires a version 1.0 purelib py3-none-any wheel");
    }
    let metadata = read(files, &format!("{dist_info}/METADATA")).await?;
    let metadata = headers(&metadata)?;
    let (name, version) = exact_requirement(&format!(
        "{}=={}",
        scalar(&metadata, "Name")?,
        scalar(&metadata, "Version")?,
    ))?;
    if name != package.name || version != package.version {
        bail!("Python wheel metadata identity mismatch for {}", package.name);
    }
    if !matches!(scalar(&metadata, "Metadata-Version")?, "2.1" | "2.2" | "2.3" | "2.4")
        || metadata.contains_key("requires-python")
        || metadata.contains_key("provides-extra")
        || metadata.contains_key("dynamic")
    {
        bail!("unsupported Python spike wheel metadata");
    }
    let record_path = format!("{dist_info}/RECORD");
    let record = read(files, &record_path).await?;
    let mut recorded = BTreeSet::new();
    for line in record.lines() {
        let fields = line.split(',').collect::<Vec<_>>();
        let [path, digest, size] = fields.as_slice() else {
            bail!("Python spike requires unquoted three-column RECORD rows");
        };
        if !recorded.insert(*path) {
            bail!("duplicate Python RECORD entry: {path}");
        }
        let Some(source) = files.get(*path) else {
            bail!("Python RECORD names a missing file: {path}");
        };
        if *path == record_path {
            if !digest.is_empty() || !size.is_empty() {
                bail!("Python RECORD must not hash itself");
            }
            continue;
        }
        let bytes = tokio::fs::read(source).await.into_diagnostic()?;
        let expected_digest = format!("sha256={}", URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes)));
        if *digest != expected_digest || size.parse::<usize>().ok() != Some(bytes.len()) {
            bail!("Python RECORD verification failed for {path}");
        }
    }
    if recorded.len() != files.len() {
        bail!("Python RECORD does not cover every wheel file");
    }
    let dependencies = metadata.get("requires-dist").cloned().unwrap_or_default();
    for requirement in &dependencies {
        exact_requirement(requirement)?;
    }
    Ok(dependencies)
}

async fn read(files: &BTreeMap<String, PathBuf>, path: &str) -> Result<String> {
    let Some(source) = files.get(path) else {
        bail!("Python wheel is missing {path}");
    };
    tokio::fs::read_to_string(source).await.into_diagnostic()
}

fn headers(contents: &str) -> Result<BTreeMap<String, Vec<String>>> {
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for line in contents.lines().take_while(|line| !line.is_empty()) {
        if line.starts_with(char::is_whitespace) {
            bail!("Python spike does not support folded metadata headers");
        }
        let Some((name, value)) = line.split_once(':') else {
            bail!("invalid Python metadata header");
        };
        headers.entry(name.to_ascii_lowercase()).or_default().push(value.trim().to_string());
    }
    Ok(headers)
}

fn scalar<'a>(headers: &'a BTreeMap<String, Vec<String>>, key: &str) -> Result<&'a str> {
    match headers.get(&key.to_ascii_lowercase()).map(Vec::as_slice) {
        Some([value]) => Ok(value),
        _ => bail!("Python metadata requires exactly one {key} header"),
    }
}
