use super::{
    super::wire::{error_frame, error_frame_with_code, ndjson_single_frame, pypi_done_frame},
    IndexReader, PypiResolveRequest, Resolver, resolve,
};
use axum::response::Response;
use pnpm_python_resolver::{Inputs, Lockfile};

pub(super) async fn resolve_request(
    runtime: &Resolver,
    identity: pnpr_policy::Identity,
    index: url::Url,
    request: PypiResolveRequest,
    requirements: Vec<pep508_rs::Requirement>,
) -> Response {
    let reader = IndexReader::new(runtime, identity, index);
    let inputs = Inputs::new(&requirements, &request.target, reader.index.as_str());
    match resolve(&reader, &requirements, &request.target).await {
        Ok(packages) => {
            let solution = packages.0;
            match Lockfile::new(
                &packages.1,
                &request.target,
                &requirements,
                solution,
                inputs,
                request.requires_python,
            ) {
                Ok(lockfile) => ndjson_single_frame(&pypi_done_frame(&lockfile)),
                Err(err) => ndjson_single_frame(&error_frame(&super::super::report_message(&err))),
            }
        }
        Err(err) => {
            let code = err
                .contains("must be resolved by the client")
                .then_some("ERR_PNPM_PYPI_CLIENT_RESOLVE_REQUIRED");
            ndjson_single_frame(&error_frame_with_code(&err, code))
        }
    }
}
