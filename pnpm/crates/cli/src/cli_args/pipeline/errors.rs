use derive_more::{Display, Error};
use miette::Diagnostic;
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PipelineError {
    #[display("No pipelines are defined in pnpm-workspace.yaml")]
    #[diagnostic(
        code(ERR_PNPM_NO_PIPELINES),
        help(
            "Declare one under the \"pipelines\" key, e.g.\n\npipelines:\n  check:\n    - lint\n    - build\n    - test"
        )
    )]
    NoPipelines,

    #[display("There is no pipeline named \"{name}\". Available pipelines: {available}")]
    #[diagnostic(code(ERR_PNPM_UNKNOWN_PIPELINE))]
    UnknownPipeline { name: String, available: String },

    #[display("\"pnpm pipeline\" failed in {count} tasks")]
    #[diagnostic(code(ERR_PNPM_PIPELINE_FAIL))]
    PipelineFail {
        #[error(not(source))]
        count: usize,
    },
}
