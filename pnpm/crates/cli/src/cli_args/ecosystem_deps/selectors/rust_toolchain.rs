//! `rust` and `!rust` in `pnpm update`, which name the Rust toolchain where
//! the Cargo integration is enabled.

use super::{
    BareUpdateNames,
    ParsedUpdateSelectors,
    parse_bare_update_selector,
};
use pnpm_config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustToolchainSelector {
    Include,
    Exclude,
}

/// `rust` selects only the toolchain. `!rust` also stays an exclusion of
/// the packages named `rust`, as any other bare exclusion is.
pub(super) fn parse_unprefixed_update_selector(
    raw: &str,
    (bare_names, config): (BareUpdateNames, &Config),
    parsed: &mut ParsedUpdateSelectors,
) {
    if matches!(bare_names, BareUpdateNames::NodeOnly) {
        parsed.node_selectors.push(raw.to_string());
        return;
    }
    let toolchain = match raw {
        "rust" if config.cargo.enabled => Some(RustToolchainSelector::Include),
        "!rust" if config.cargo.enabled => Some(RustToolchainSelector::Exclude),
        _ => None,
    };
    if toolchain.is_some() {
        parsed.rust_toolchain = toolchain;
    }
    if toolchain != Some(RustToolchainSelector::Include) {
        parse_bare_update_selector(raw, parsed);
    }
}
