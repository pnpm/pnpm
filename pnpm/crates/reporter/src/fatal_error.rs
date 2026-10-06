use serde::Serialize;

use crate::{LogLevel, NdjsonReporter, SkippedOptionalParent, emit_record};

/// Fatal error record consumed by pnpm's log reporters.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FatalErrorLog {
    pub name: &'static str,
    pub level: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    pub prefix: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pkgs_stack: Vec<SkippedOptionalParent>,
}

impl NdjsonReporter {
    pub fn emit_fatal_error(error: &FatalErrorLog) {
        emit_record(error);
    }
}
