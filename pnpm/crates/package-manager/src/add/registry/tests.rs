use super::{PickedRange, registry_specifier_to_save};
use pnpm_config::CatalogMode;

#[test]
fn strict_catalogs_pin_tag_selectors_to_the_picked_version() {
    let picked =
        PickedRange { range: "^2.7.16".to_string(), version: "2.7.16".to_string(), is_tag: true };
    assert_eq!(registry_specifier_to_save(&picked, CatalogMode::Strict), "2.7.16");
    assert_eq!(registry_specifier_to_save(&picked, CatalogMode::Prefer), "^2.7.16");
}

#[test]
fn strict_catalogs_keep_range_selectors() {
    let picked =
        PickedRange { range: "^2.7.16".to_string(), version: "2.7.16".to_string(), is_tag: false };
    assert_eq!(registry_specifier_to_save(&picked, CatalogMode::Strict), "^2.7.16");
}
