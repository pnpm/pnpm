use pnpm_network::redact_and_sanitize;
use pnpm_reporter::{FatalErrorLog, LogLevel, SkippedOptionalParent};
use pnpm_resolving_deps_resolver::DependencyResolutionError;
use std::{
    path::Path,
    sync::{Mutex, PoisonError},
};

static PREFIX: Mutex<String> = Mutex::new(String::new());

pub(crate) fn set_prefix(directory: &Path) {
    let directory = dunce::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    *PREFIX.lock().unwrap_or_else(PoisonError::into_inner) = directory.display().to_string();
}

pub(crate) fn error_log(error: &miette::Report) -> FatalErrorLog {
    let context = error.chain().find_map(|cause| cause.downcast_ref::<DependencyResolutionError>());
    FatalErrorLog {
        name: "pnpm",
        level: LogLevel::Error,
        code: error.code().map(|code| code.to_string()),
        message: match context {
            Some(context) => context.to_string(),
            None => pnpm_diagnostics::collapsed_message(error.as_ref()),
        },
        prefix: match context {
            Some(context) => context.prefix.clone(),
            None => PREFIX
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        },
        pkgs_stack: context
            .map(|context| {
                context.parents
                    .iter()
                    .map(|parent| SkippedOptionalParent {
                        id: redact_and_sanitize(&parent.id),
                        name: redact_and_sanitize(&parent.name),
                        version: redact_and_sanitize(&parent.version),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}
