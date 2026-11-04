//! `--variant-integrity`: the archives of a version that other machines
//! uploaded with `--upload-variant`, given by the integrity each printed
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#uploading-variants-from-several-machines)).

use pnpm_pack::variants::{VariantDeclaration, VariantEntry};
use pnpm_publish::{PackedVariant, StagedReference, VariantArchive};

/// One `--variant-integrity` value: `<target>=<integrity>`, or
/// `<name>~<target>=<integrity>` naming its package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GivenIntegrity {
    pub(super) package: Option<String>,
    pub(super) target: String,
    pub(super) integrity: String,
}

/// Parse `--variant-integrity` values. A value splits at its first `=`,
/// since targets never contain one, and its package at the last `~` before
/// it, since targets never contain one either.
pub(super) fn parse(values: &[String]) -> miette::Result<Vec<GivenIntegrity>> {
    values
        .iter()
        .map(|value| {
            let parsed = value
                .split_once('=')
                .filter(|(_, integrity)| !integrity.is_empty())
                .map(|(named, integrity)| {
                    let (package, target) = match named.rsplit_once('~') {
                        Some((package, target)) => (Some(package.to_string()), target),
                        None => (None, named),
                    };
                    GivenIntegrity { package, target: target.to_string(), integrity: integrity.to_string() }
                })
                .filter(|given| !given.target.is_empty());
            parsed.ok_or_else(|| {
                miette::miette!(
                    code = "ERR_PNPM_INVALID_VARIANT_INTEGRITY",
                    help = "Pass each value pnpm publish --upload-variant printed: <target>=<integrity>, or <name>~<target>=<integrity> with --recursive. An empty value is a job output that no upload job set.",
                    "Invalid --variant-integrity value {value:?}",
                )
            })
        })
        .collect()
}

/// Check the values of a recursive publish against the names the selected
/// projects publish under, `packages`: each value names its package, as a
/// value without one would apply to every package, and exactly one selected
/// project publishes under it, as none would leave a package to be packed
/// here instead of published from its uploaded archives, and two would both
/// take its archives.
pub(super) fn check_recursive(given: &[GivenIntegrity], packages: &[&str]) -> miette::Result<()> {
    if let Some(unnamed) = given.iter().find(|given| given.package.is_none()) {
        return Err(miette::miette!(
            code = "ERR_PNPM_INVALID_VARIANT_INTEGRITY",
            help = "With --recursive, pass each value as <name>~<target>=<integrity>.",
            "--variant-integrity value for {:?} names no package",
            unnamed.target,
        ));
    }
    for package in given.iter().filter_map(|given| given.package.as_deref()) {
        match packages
            .iter()
            .filter(|selected| **selected == package)
            .count()
        {
            1 => {}
            0 => {
                return Err(miette::miette!(
                    code = "ERR_PNPM_UNKNOWN_VARIANT_PACKAGE",
                    "--variant-integrity names {package:?}, which this recursive publish does not select",
                ));
            }
            _ => {
                return Err(miette::miette!(
                    code = "ERR_PNPM_AMBIGUOUS_VARIANT_PACKAGE",
                    help = "Select only one of them, or give each its own publishConfig.name.",
                    "--variant-integrity names {package:?}, which several selected projects publish as",
                ));
            }
        }
    }
    Ok(())
}

/// The archives of `package` that `given` names, against its
/// `declaration`: the `any` archive, and every platform variant by its tag.
/// `None` when `given` names none of this package's archives. Every entry
/// must be given exactly once.
pub(super) fn archives_of(
    package: &str,
    declaration: &VariantDeclaration,
    given: &[GivenIntegrity],
) -> miette::Result<Option<(Option<StagedReference>, Vec<PackedVariant>)>> {
    let ours: Vec<&GivenIntegrity> = given
        .iter()
        .filter(|given| {
            given.package
                .as_deref()
                .is_none_or(|named| named == package)
        })
        .collect();
    if ours.is_empty() {
        return Ok(None);
    }
    let by_entry = values_by_entry(package, declaration, &ours)?;
    let mut any = None;
    let mut variants = Vec::new();
    for (entry, given) in declaration.entries.iter().zip(by_entry) {
        let Some(given) = given else {
            let label = entry.label();
            return Err(miette::miette!(
                code = "ERR_PNPM_MISSING_VARIANT_INTEGRITY",
                help = "Pass the value the job that uploads this archive printed. A job that failed or was skipped printed none.",
                "--variant-integrity gives no archive for {label} of {package}",
            ));
        };
        let reference =
            StagedReference { integrity: given.integrity.clone(), label: entry.label() };
        match &entry.tag {
            Some(tag) => variants.push(PackedVariant {
                tag: tag.clone(),
                archive: VariantArchive::Staged(reference),
            }),
            None => any = Some(reference),
        }
    }
    Ok(Some((any, variants)))
}

/// The value `given` holds for each entry of `declaration`, by entry index.
/// A value that names no entry, or a second value for one entry, fails.
fn values_by_entry<'a>(
    package: &str,
    declaration: &VariantDeclaration,
    given: &[&'a GivenIntegrity],
) -> miette::Result<Vec<Option<&'a GivenIntegrity>>> {
    let mut by_entry: Vec<Option<&GivenIntegrity>> = vec![None; declaration.entries.len()];
    for given in given {
        let entry = declaration
            .find(&given.target)
            .map_err(|error| {
                miette::miette!(
                    code = "ERR_PNPM_UNKNOWN_VARIANT",
                    "--variant-integrity names {:?} of {package}: {error}",
                    given.target,
                )
            })?;
        let index = entry_index(declaration, entry);
        if let Some(earlier) = by_entry[index].replace(given) {
            let label = entry.label();
            return Err(miette::miette!(
                code = "ERR_PNPM_DUPLICATE_VARIANT_INTEGRITY",
                "--variant-integrity names the archive for {label} of {package} twice, as {:?} and {:?}",
                earlier.target,
                given.target,
            ));
        }
    }
    Ok(by_entry)
}

fn entry_index(declaration: &VariantDeclaration, entry: &VariantEntry) -> usize {
    declaration.entries
        .iter()
        .position(|candidate| candidate == entry)
        .expect("the entry is one of the declaration's")
}

#[cfg(test)]
mod tests;
