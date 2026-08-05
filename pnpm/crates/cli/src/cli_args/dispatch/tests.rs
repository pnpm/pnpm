use super::json_error_message;
use crate::cli_args::pack::PACK_ERROR_CONTEXT;
use miette::Diagnostic;

#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("canonical publish failure")]
#[diagnostic(code(ERR_PNPM_TEST_JSON_ERROR))]
struct CanonicalError {
    #[error(source)]
    source: SensitiveCause,
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("registry response included token=secret")]
struct SensitiveCause;

#[test]
fn json_error_message_omits_nested_causes() {
    let error = miette::Report::new(CanonicalError { source: SensitiveCause });
    let message = json_error_message(&error);

    assert_eq!(message, "canonical publish failure");
    assert!(!message.contains("token=secret"));
}

#[test]
fn json_error_message_unwraps_pack_context() {
    let error =
        miette::Report::new(CanonicalError { source: SensitiveCause }).wrap_err(PACK_ERROR_CONTEXT);

    assert_eq!(json_error_message(&error), "canonical publish failure");
}
