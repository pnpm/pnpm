use super::{
    PackageProviderError, PackageProviderOutput, ProviderRequestBundle, ProviderResponse,
    types::PROTOCOL_VERSION,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) fn invoke_provider(
    provider: &str,
    request_json: Vec<u8>,
) -> Result<Vec<u8>, PackageProviderError> {
    let mut child = Command::new(provider)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|source| PackageProviderError::Spawn { provider: provider.to_string(), source })?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let writer = std::thread::spawn(move || {
        use std::io::Write as _;
        let _ = stdin.write_all(&request_json);
    });
    let output = child
        .wait_with_output()
        .map_err(|source| PackageProviderError::Spawn { provider: provider.to_string(), source })?;
    writer.join().expect("provider stdin writer must not panic");
    if !output.status.success() {
        return Err(PackageProviderError::NonZeroExit {
            provider: provider.to_string(),
            code: output.status
                .code()
                .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        });
    }
    Ok(output.stdout)
}

pub(crate) fn parse_provider_response(
    provider: &str,
    stdout: &[u8],
) -> Result<ProviderResponse, PackageProviderError> {
    let response: ProviderResponse = serde_json::from_slice(stdout)
        .map_err(|_| PackageProviderError::InvalidJson { provider: provider.to_string() })?;
    if response.protocol != Some(PROTOCOL_VERSION) || response.paths.is_none() {
        return Err(PackageProviderError::UnsupportedResponse {
            provider: provider.to_string(),
            protocol: response.protocol.map_or_else(
                || "missing".to_string(),
                |protocol| protocol.to_string(),
            ),
        });
    }
    Ok(response)
}

pub(crate) fn validate_provider_response(
    bundle: &ProviderRequestBundle,
    response: ProviderResponse,
) -> Result<PackageProviderOutput, PackageProviderError> {
    let paths = response.paths.expect("checked by parse_provider_response");
    let mut skipped_keys = Vec::new();
    let mut skipped_dep_paths: HashSet<String> = HashSet::new();
    for dep_path in response.skipped.unwrap_or_default() {
        let is_optional = bundle.request.nodes
            .get(&dep_path)
            .is_some_and(|node| node.optional == Some(true));
        if !is_optional {
            return Err(PackageProviderError::SkippedNonOptional { dep_path });
        }
        skipped_keys.push(bundle.key_by_dep_path[&dep_path].clone());
        skipped_dep_paths.insert(dep_path);
    }
    let mut out_paths = HashMap::with_capacity(bundle.key_by_dep_path.len());
    for (dep_path, key) in &bundle.key_by_dep_path {
        if skipped_dep_paths.contains(dep_path) {
            continue;
        }
        let Some(dir) = paths.get(dep_path) else {
            return Err(PackageProviderError::MissingPath { dep_path: dep_path.clone() });
        };
        if dir.is_empty() || !Path::new(dir).has_root() {
            return Err(PackageProviderError::RelativePath {
                dep_path: dep_path.clone(),
                dir: dir.clone(),
            });
        }
        out_paths.insert(key.clone(), PathBuf::from(dir));
    }
    Ok(PackageProviderOutput { paths: out_paths, skipped: skipped_keys })
}
