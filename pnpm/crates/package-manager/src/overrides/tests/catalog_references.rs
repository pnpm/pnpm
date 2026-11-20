use super::{
    pairs,
    prod_only,
    rewritten,
};
use crate::{
    overrides_catalog_reference_rewriter,
    overrides_dependency_rewriter,
};
use indexmap::IndexMap;
use pnpm_catalogs_types::{
    Catalog,
    Catalogs,
};
use pnpm_config::Config;
use std::path::Path;

fn config_with_overrides(overrides: &[(&str, &str)]) -> Config {
    let mut config = Config::new();
    config.overrides = Some(
        overrides
            .iter()
            .map(|(selector, value)| ((*selector).to_string(), (*value).to_string()))
            .collect::<IndexMap<_, _>>(),
    );
    config
}

fn local_catalog() -> Catalogs {
    let mut local = Catalog::new();
    local.insert("lib".to_string(), "^2.0.0".to_string());
    let mut catalogs = Catalogs::new();
    catalogs.insert("local".to_string(), local);
    catalogs
}

#[test]
fn catalog_reference_rewriter_keeps_the_catalog_reference_of_an_override() {
    let config = config_with_overrides(&[("lib", "catalog:local"), ("other", "^3.0.0")]);
    let catalogs = local_catalog();
    let declared = || prod_only(&[("lib", "^1.0.0"), ("other", "^1.0.0")]);

    let reference_rewriter =
        overrides_catalog_reference_rewriter(&config, &catalogs, Path::new("/workspace"))
            .expect("parse overrides")
            .expect("overrides are configured");
    assert_eq!(
        rewritten(&reference_rewriter, "app", declared()),
        pairs(&[("lib", "catalog:local"), ("other", "^3.0.0")]),
    );

    let resolving_rewriter =
        overrides_dependency_rewriter(&config, &catalogs, Path::new("/workspace"))
            .expect("parse overrides")
            .expect("overrides are configured");
    assert_eq!(
        rewritten(&resolving_rewriter, "app", declared()),
        pairs(&[("lib", "^2.0.0"), ("other", "^3.0.0")]),
    );
}

#[test]
fn catalog_reference_rewriter_is_none_without_overrides() {
    let rewriter =
        overrides_catalog_reference_rewriter(&Config::new(), &local_catalog(), Path::new("/w"))
            .expect("parse overrides");
    assert!(rewriter.is_none());
}
