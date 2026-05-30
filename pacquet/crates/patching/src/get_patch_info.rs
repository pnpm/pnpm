use crate::types::{ExtendedPatchInfo, PatchGroupRecord};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};

/// Raised when a `name@version` pair satisfies more than one
/// configured version range. The user must add an exact-version
/// entry to break the tie.
///
/// Mirrors upstream's
/// [`ERR_PNPM_PATCH_KEY_CONFLICT`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/getPatchInfo.ts#L5-L19).
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Unable to choose between {n_satisfied} version ranges to patch {pkg_name}@{pkg_version}: {ranges}",
    n_satisfied = satisfied_versions.len(),
    ranges = satisfied_versions.join(", "),
)]
#[diagnostic(
    code(ERR_PNPM_PATCH_KEY_CONFLICT),
    help("Explicitly set the exact version ({pkg_name}@{pkg_version}) to resolve conflict")
)]
pub struct PatchKeyConflictError {
    pub pkg_name: String,
    pub pkg_version: String,
    pub satisfied_versions: Vec<String>,
}

/// Look up the patch (if any) that applies to `pkg_name@pkg_version`.
///
/// Ports upstream's
/// [`getPatchInfo`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/getPatchInfo.ts#L21-L40).
/// Match precedence:
///
/// 1. exact version (`exact[pkg_version]`)
/// 2. unique satisfying range (error if more than one)
/// 3. wildcard (`all`)
///
/// Returns `Ok(None)` when nothing matches the name *or* when the
/// name matches but every bucket misses. Returns `Err(...)` only on
/// rule 2's ambiguity.
///
/// If the configured ranges fail to parse as semver, those entries
/// are silently skipped — the upstream JS path goes through
/// `semver.satisfies`, which treats unparsable ranges as
/// non-matching. Pacquet matches that behavior.
pub fn get_patch_info<'a>(
    patch_file_groups: Option<&'a PatchGroupRecord>,
    pkg_name: &str,
    pkg_version: &str,
) -> Result<Option<&'a ExtendedPatchInfo>, PatchKeyConflictError> {
    let Some(groups) = patch_file_groups else {
        return Ok(None);
    };
    let Some(group) = groups.get(pkg_name) else {
        return Ok(None);
    };

    if let Some(exact) = group.exact.get(pkg_version) {
        return Ok(Some(exact));
    }

    let Ok(parsed_version) = Version::parse(pkg_version) else {
        // Non-semver target version: only exact-string and wildcard
        // can match. `pkg_version` already missed the exact bucket
        // above, so fall through to `all`.
        return Ok(group.all.as_ref());
    };

    let satisfied: Vec<&'a crate::types::PatchGroupRangeItem> = group
        .range
        .iter()
        .filter(|item| match Range::parse(&item.version) {
            Ok(range) => range.satisfies(&parsed_version),
            Err(_) => false,
        })
        .collect();

    if satisfied.len() > 1 {
        return Err(PatchKeyConflictError {
            pkg_name: pkg_name.to_string(),
            pkg_version: pkg_version.to_string(),
            satisfied_versions: satisfied.iter().map(|item| item.version.clone()).collect(),
        });
    }
    if let [only] = satisfied.as_slice() {
        return Ok(Some(&only.patch));
    }

    Ok(group.all.as_ref())
}

#[cfg(test)]
mod tests;
