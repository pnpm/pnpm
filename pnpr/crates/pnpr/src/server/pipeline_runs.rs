use super::{
    AppState, AuthedCaller, Identity, Path, RegistryError, Response, State, StatusCode, header,
    not_found, private_no_cache, require_caller,
};
use axum::response::IntoResponse;

/// `PUT /-/pnpr/v0/pipeline/runs` — record one `pnpm pipeline` run. The
/// document is stored verbatim; identifiers are validated by the store.
pub(super) async fn serve_publish_pipeline_run(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    if let Err(err) = require_caller(&identity, "pipeline run publication") {
        return private_no_cache(err.into_response());
    }
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
    if let Err(error) = authorize_pipeline_workspace(&state, &identity, &run.workspace, true) {
        return private_no_cache(error.into_response());
    }
    private_no_cache(
        match state
            .inner
            .pipeline_runs
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

pub(super) async fn serve_list_pipeline_runs(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    uri: axum::http::Uri,
) -> Response {
    if let Err(err) = require_caller(&identity, "pipeline runs") {
        return private_no_cache(err.into_response());
    }
    let (workspace, limit) = parse_pipeline_run_query(uri.query().unwrap_or_default());
    if let Some(workspace) = &workspace
        && let Err(error) = authorize_pipeline_workspace(&state, &identity, workspace, false)
    {
        return private_no_cache(error.into_response());
    }
    let store = state.inner.pipeline_runs.as_ref().expect("pipeline routes require a run store");
    let visible: Vec<&str> = state
        .inner
        .config
        .pipeline
        .workspaces
        .iter()
        .filter(|(name, policy)| {
            policy.access.allows(&identity)
                && workspace.as_ref().is_none_or(|requested| requested == *name)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    private_no_cache(match store.list(&visible, limit).await {
        Ok(runs) => axum::Json(serde_json::json!({ "runs": runs })).into_response(),
        Err(error) => error.into_response(),
    })
}

/// `GET /-/pnpr/v0/pipeline/runs[?workspace=&limit=]` — the most recent
/// run summaries, newest first.
/// The workspace filter and page size a run listing asks for.
pub(super) fn parse_pipeline_run_query(query: &str) -> (Option<String>, usize) {
    let mut workspace = None;
    let mut limit: usize = 50;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "workspace" if !value.is_empty() => workspace = Some(value.into_owned()),
            "limit" => limit = value.parse().unwrap_or(limit),
            _ => {}
        }
    }
    (workspace, limit.clamp(1, pnpr_pipeline_runs::MAX_LIST_RUNS))
}

/// `GET /-/pnpr/v0/pipeline/runs/{workspace}/{run_id}` — one run's full
/// record, event stream included.
pub(super) async fn serve_get_pipeline_run(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((workspace, run_id)): Path<(String, String)>,
) -> Response {
    if let Err(error) = authorize_pipeline_workspace(&state, &identity, &workspace, false) {
        return private_no_cache(error.into_response());
    }
    let store = state.inner.pipeline_runs.as_ref().expect("pipeline routes require a run store");
    private_no_cache(match store.get(&workspace, &run_id).await {
        Ok(Some(run)) => axum::Json(run).into_response(),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    })
}

pub(super) fn authorize_pipeline_workspace(
    state: &AppState,
    identity: &Identity,
    workspace: &str,
    publish: bool,
) -> Result<(), RegistryError> {
    let username = require_caller(identity, "pipeline runs")?;
    let policy = state.inner.config.pipeline.workspaces.get(workspace);
    if !policy.is_some_and(|policy| policy.access.allows(identity)) {
        return Err(RegistryError::NotFound);
    }
    if publish && !policy.is_some_and(|policy| policy.publish.allows(identity)) {
        return Err(RegistryError::Forbidden {
            user: username,
            action: "publish pipeline runs",
            resource: workspace.to_string(),
        });
    }
    Ok(())
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
