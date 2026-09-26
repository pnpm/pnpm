use miette::Diagnostic;
use pnpm_resolving_npm_resolver::PickPackageError;

use super::super::workspace_resolution::map_resolve_error;

#[test]
fn keeps_the_code_and_help_of_a_pick_package_error() {
    let err = map_resolve_error(Box::new(PickPackageError::NoOfflineMeta {
        spec_name: "acme".to_string(),
        spec_fetch_spec: "^1.0.0".to_string(),
        pkg_mirror: "mirror/acme.jsonl".into(),
        hint: Some("legacy mirror hint".to_string()),
    }));
    assert_eq!(
        err.code()
            .map(|code| code.to_string())
            .as_deref(),
        Some("ERR_PNPM_NO_OFFLINE_META"),
    );
    assert_eq!(
        err.help()
            .map(|help| help.to_string())
            .as_deref(),
        Some("legacy mirror hint"),
    );
}
