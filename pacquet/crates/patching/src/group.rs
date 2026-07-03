use crate::{
    key::parse_key,
    types::{ExtendedPatchInfo, PatchGroupRangeItem, PatchGroupRecord},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Range;

/// Input to [`group_patched_dependencies`]: the
/// `pnpm.patchedDependencies` map after hashes have been computed.
///
/// One entry per `patchedDependencies` key. The config accepts either a
/// string hash (the historical shape) or a `{ hash, patchFilePath }`
/// object. Pacquet collapses both into the latter at config-load time,
/// so the grouper only sees the resolved shape.
#[derive(Debug, Clone)]
pub struct PatchInput {
    pub hash: String,
    pub patch_file_path: Option<std::path::PathBuf>,
}

/// Raised when a `patchedDependencies` key's version segment is
/// non-empty, not a valid semver version, and not a valid semver
/// range. Surfaces as `ERR_PNPM_PATCH_NON_SEMVER_RANGE`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{non_semver_version} is not a valid semantic version range.")]
#[diagnostic(code(ERR_PNPM_PATCH_NON_SEMVER_RANGE))]
pub struct PatchNonSemverRangeError {
    pub non_semver_version: String,
}

/// Bucketize `patchedDependencies` by package name and match flavor.
pub fn group_patched_dependencies<Iter>(
    entries: Iter,
) -> Result<PatchGroupRecord, PatchNonSemverRangeError>
where
    Iter: IntoIterator<Item = (String, PatchInput)>,
{
    let mut result: PatchGroupRecord = PatchGroupRecord::new();

    for (key, info) in entries {
        let parsed = parse_key(&key);
        let extended = ExtendedPatchInfo {
            hash: info.hash,
            patch_file_path: info.patch_file_path,
            key: key.clone(),
        };

        match (parsed.name, parsed.version, parsed.non_semver_version) {
            (Some(name), Some(version), _) => {
                let group = result.entry(name.to_string()).or_default();
                group.exact.insert(version.to_string(), extended);
            }
            (Some(name), None, Some(non_semver_version)) => {
                if Range::parse(non_semver_version).is_err() {
                    return Err(PatchNonSemverRangeError {
                        non_semver_version: non_semver_version.to_string(),
                    });
                }
                let group = result.entry(name.to_string()).or_default();
                if non_semver_version.trim() == "*" {
                    group.all = Some(extended);
                } else {
                    group.range.push(PatchGroupRangeItem {
                        version: non_semver_version.to_string(),
                        patch: extended,
                    });
                }
            }
            _ => {
                // A bare `name` key and a `name@*` key both target
                // `group.all`. This bare-name branch runs last, so a
                // bare `name` overwrites whatever `name@*` set.
                let group = result.entry(key.clone()).or_default();
                group.all = Some(extended);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests;
