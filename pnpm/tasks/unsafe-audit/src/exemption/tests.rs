use super::{Tier, exempt, exemption_for};

#[test]
fn every_exemption_is_looked_up_by_name() {
    for exemption in exempt() {
        let found = exemption_for(exemption.crate_name).expect("the exemption is found");
        assert_eq!(found.crate_name, exemption.crate_name);
    }
    assert!(exemption_for("pnpm-cli").is_none());
}

#[test]
fn no_crate_is_exempt_twice() {
    let mut names: Vec<&str> = exempt()
        .iter()
        .map(|exemption| exemption.crate_name)
        .collect();
    let listed = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), listed, "a crate is exempt twice");
}

/// Only a `-sys` crate may declare a foreign ABI, and the audit decides
/// that from the tier, so the tier has to match the name.
#[test]
fn the_foreign_abi_tier_holds_only_sys_crates() {
    for exemption in exempt() {
        assert_eq!(
            exemption.tier == Tier::ForeignAbi,
            exemption.crate_name.ends_with("-sys"),
            "{} is in the wrong tier for its name",
            exemption.crate_name,
        );
    }
}

/// The reason is what makes the list readable rather than a bare
/// allowlist, so an empty one is not an entry.
#[test]
fn every_exemption_gives_a_reason() {
    for exemption in exempt() {
        assert!(!exemption.reason.is_empty(), "{} gives no reason", exemption.crate_name);
    }
}
