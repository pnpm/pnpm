use super::{
    super::{
        json_error, json_error_with_code, parse_request,
        protocol::PypiResolveRequest,
        request_validation::forbidden_off_allowlist,
        wire::{error_frame, error_frame_with_code, ndjson_single_frame, pypi_done_frame},
    },
    IndexReader, MAX_REQUIREMENTS, MAX_TAGS, Resolver, index_url, resolve,
};
use axum::{http::StatusCode, response::Response};
use pnpm_python_resolver::{Inputs, Lockfile, parse_requirement};
use pnpr_route::url_has_inline_credentials;

pub(super) struct ValidatedPypiRequest {
    index: url::Url,
    request: PypiResolveRequest,
    requirements: Vec<pep508_rs::Requirement>,
}

pub(super) fn validate_request(
    runtime: &Resolver,
    body: axum::body::Bytes,
) -> Result<ValidatedPypiRequest, Response> {
    let request: PypiResolveRequest = parse_request(&body)?;
    drop(body);
    let index = index_url(&request.index).map_err(|err| json_error(StatusCode::BAD_REQUEST, &err))?;
    if let Some(response) = reject_unusable_request(runtime, &request, &index) {
        return Err(response);
    }
    let requirements = parse_index_requirements(&request)?;
    Ok(ValidatedPypiRequest { index, request, requirements })
}

pub(super) fn parse_index_requirements(
    request: &PypiResolveRequest,
) -> Result<Vec<pep508_rs::Requirement>, Response> {
    let requirements = request.requirements
        .iter()
        .map(|requirement| parse_requirement(requirement))
        .collect::<miette::Result<Vec<_>>>()
        .map_err(|err| json_error(StatusCode::BAD_REQUEST, &super::super::report_message(&err)))?;
    if requirements
        .iter()
        .any(|requirement| {
            matches!(requirement.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_)))
        })
    {
        return Err(json_error_with_code(
            StatusCode::BAD_REQUEST,
            "Python direct URL requirements must be resolved by the client",
            Some("ERR_PNPR_PYPI_CLIENT_RESOLVE_REQUIRED"),
        ));
    }
    Ok(requirements)
}

pub(super) fn reject_unusable_request(
    runtime: &Resolver,
    request: &PypiResolveRequest,
    index: &url::Url,
) -> Option<Response> {
    if url_has_inline_credentials(index.as_str()) {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            "inline URL credentials (user:pass@host) are not allowed; \
             configure an upstream credential alias instead",
        ));
    }
    if !runtime.route_context.allows_registry(index.as_str()) {
        return Some(forbidden_off_allowlist(index.as_str()));
    }
    if request.requirements.len() > MAX_REQUIREMENTS {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            &format!("a resolve request may name at most {MAX_REQUIREMENTS} requirements"),
        ));
    }
    if request.target.tags.len() > MAX_TAGS {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            &format!("a resolve request may name at most {MAX_TAGS} wheel tags"),
        ));
    }
    None
}

pub(super) async fn resolve_request(
    runtime: &Resolver,
    identity: pnpr_policy::Identity,
    validated: ValidatedPypiRequest,
) -> Response {
    let ValidatedPypiRequest { index, request, requirements } = validated;
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
                .then_some("ERR_PNPR_PYPI_CLIENT_RESOLVE_REQUIRED");
            ndjson_single_frame(&error_frame_with_code(&err, code))
        }
    }
}
