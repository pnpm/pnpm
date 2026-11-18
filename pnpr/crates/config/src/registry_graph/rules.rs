use super::super::{PackagePattern, PackageRules};
use pnpr_policy::RuleTable;

/// The rules of a registry that claims `patterns`. No grant covers it until
/// the server applies the stored ones.
pub(super) fn claimed(patterns: impl IntoIterator<Item = PackagePattern>) -> PackageRules {
    PackageRules::from_table(RuleTable::of_grants(patterns.into_iter().collect(), Vec::new()))
}
