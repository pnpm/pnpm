use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_resolving_npm_resolver::PickPackageError;
use pnpm_resolving_resolver_base::UnsupportedProtocolError;
use std::{error::Error as _, iter};

use super::super::{
    failed_edge::is_droppable_resolve_error, workspace_resolution::map_resolve_error,
};

#[derive(Debug, Display, Error)]
#[display("Failed to fetch SHASUMS256.txt to verify the Node.js download")]
struct FetchFailed {
    #[error(source)]
    cause: CertificateRejected,
}

#[derive(Debug, Display, Error)]
#[display("invalid peer certificate: UnknownIssuer")]
struct CertificateRejected;

#[test]
fn keeps_the_causes_of_an_unrecognized_resolver_error() {
    let err = map_resolve_error(Box::new(FetchFailed { cause: CertificateRejected }));

    assert_eq!(
        err.to_string(),
        "Failed to resolve dependency: Failed to fetch SHASUMS256.txt to verify the Node.js download",
    );
    let causes: Vec<String> =
        iter::successors(err.source(), |&cause| cause.source()).map(ToString::to_string).collect();
    assert_eq!(
        causes,
        [
            "Failed to fetch SHASUMS256.txt to verify the Node.js download",
            "invalid peer certificate: UnknownIssuer",
        ],
    );
    assert!(is_droppable_resolve_error(&err), "an optional edge must still skip it");
}

#[test]
fn keeps_the_code_and_help_of_a_pick_package_error_and_stays_droppable() {
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
    assert!(is_droppable_resolve_error(&err), "an optional edge must still skip it");
}

#[test]
fn keeps_the_code_of_an_unsupported_protocol_error_and_stays_droppable() {
    let unsupported =
        UnsupportedProtocolError::detect("patch:got@npm%3A11.8.2#~/x.patch").expect("detected");
    let err = map_resolve_error(Box::new(unsupported));
    assert_eq!(
        err.code()
            .map(|code| code.to_string())
            .as_deref(),
        Some("ERR_PNPM_UNSUPPORTED_PROTOCOL"),
    );
    assert!(is_droppable_resolve_error(&err), "an optional edge must still skip it");
}
