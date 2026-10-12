use super::{
    AppState, AuthedCaller, CallerOrgs, Path, RegistryError, Response, State, StatusCode, header,
    not_found, private_no_cache,
};
use axum::response::IntoResponse;

/// `PUT /-/pnpr/v0/pipeline/runs` — record one `pnpm pipeline` run under its
/// organization. The document is stored verbatim; identifiers are validated
/// by the store.
pub(super) async fn serve_publish_pipeline_run(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let caller = match CallerOrgs::new(&state, &identity, "pipeline run publication") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let run: pnpr_pipeline_runs::PublishPipelineRun = match serde_json::from_slice(&body) {
        Ok(run) => run,
        Err(err) => {
            return private_no_cache(
                RegistryError::BadRequest {
                    reason: format!("invalid pipeline run document: {err}"),
                }
                .into_response(),
            );
        }
    };
    if let Err(error) = caller.authorize(&run.org, true, "publish pipeline runs to") {
        return private_no_cache(error.into_response());
    }
    private_no_cache(
        match state.inner.builds.pipeline_runs
            .as_ref()
            .expect("pipeline routes require a run store")
            .publish(&run)
            .await
        {
            Ok(()) => StatusCode::CREATED.into_response(),
            Err(err) => err.into_response(),
        },
    )
}

/// `GET /-/pnpr/v0/pipeline/runs[?org=&workspace=&limit=]` — the most recent
/// run summaries the caller may read, newest first.
pub(super) async fn serve_list_pipeline_runs(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    uri: axum::http::Uri,
) -> Response {
    let caller = match CallerOrgs::new(&state, &identity, "pipeline runs") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let query = parse_pipeline_run_query(uri.query().unwrap_or_default());
    if let Some(org) = &query.org
        && let Err(error) = caller.authorize(org, false, "read pipeline runs of")
    {
        return private_no_cache(error.into_response());
    }
    let store =
        state.inner.builds.pipeline_runs.as_ref().expect("pipeline routes require a run store");
    let visible: Vec<&str> = caller
        .readable()
        .filter(|org| {
            query.org
                .as_deref()
                .is_none_or(|requested| requested == *org)
        })
        .collect();
    private_no_cache(match store.list(&visible, query.workspace.as_deref(), query.limit).await {
        Ok(runs) => axum::Json(serde_json::json!({ "runs": runs })).into_response(),
        Err(error) => error.into_response(),
    })
}

/// The filters and page size a run listing asks for.
pub(super) struct PipelineRunQuery {
    pub(super) org: Option<String>,
    pub(super) workspace: Option<String>,
    pub(super) limit: usize,
}

pub(super) fn parse_pipeline_run_query(query: &str) -> PipelineRunQuery {
    let mut parsed = PipelineRunQuery { org: None, workspace: None, limit: 50 };
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "org" if !value.is_empty() => parsed.org = Some(value.into_owned()),
            "workspace" if !value.is_empty() => parsed.workspace = Some(value.into_owned()),
            "limit" => parsed.limit = value.parse().unwrap_or(parsed.limit),
            _ => {}
        }
    }
    parsed.limit = parsed.limit.clamp(1, pnpr_pipeline_runs::MAX_LIST_RUNS);
    parsed
}

/// `GET /-/pnpr/v0/pipeline/runs/{org}/{workspace}/{run_id}` — one run's full
/// record, event stream included.
pub(super) async fn serve_get_pipeline_run(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((org, workspace, run_id)): Path<(String, String, String)>,
) -> Response {
    let authorized = CallerOrgs::new(&state, &identity, "pipeline runs")
        .and_then(|caller| caller.authorize(&org, false, "read pipeline runs of"));
    if let Err(error) = authorized {
        return private_no_cache(error.into_response());
    }
    let store =
        state.inner.builds.pipeline_runs.as_ref().expect("pipeline routes require a run store");
    private_no_cache(match store.get(&org, &workspace, &run_id).await {
        Ok(Some(run)) => axum::Json(run).into_response(),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    })
}

/// `GET /-/pnpr/v0/pipeline` — a self-contained viewer over the run
/// endpoints. Static HTML with no data of its own: the reads it issues
/// carry the token the visitor pastes, so the page itself needs no auth.
pub(super) async fn serve_pipeline_ui() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("pipeline_ui.html"),
    )
        .into_response()
}
