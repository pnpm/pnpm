use super::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;

pub(crate) async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(err) => {
                tracing::error!(error = %err, "failed to install SIGINT (ctrl_c) handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to install SIGTERM signal handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("SIGINT (ctrl_c) received, starting graceful shutdown"),
        () = terminate => tracing::info!("SIGTERM received, starting graceful shutdown"),
    }
}

pub(crate) async fn serve_ping() -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response()
}

fn shutting_down_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "status": "shutting_down",
        })),
    )
        .into_response()
}

pub(crate) async fn serve_ready(State(state): State<AppState>) -> Response {
    if state.is_shutting_down() {
        return shutting_down_response();
    }

    let storage_status = state.inner.storage.check_readiness().await;
    let auth_status = state.inner.identity.auth.check_readiness().await;

    if state.is_shutting_down() {
        return shutting_down_response();
    }

    readiness_response(&storage_status, &auth_status)
}

fn readiness_response(
    storage_status: &Result<(), RegistryError>,
    auth_status: &Result<(), RegistryError>,
) -> Response {
    if storage_status.is_ok() && auth_status.is_ok() {
        return (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "status": "ok",
                "storage": "ok",
                "auth": "ok",
            })),
        )
            .into_response();
    }

    let storage_msg = probe_log_message(storage_status);
    let auth_msg = probe_log_message(auth_status);
    tracing::warn!(storage = %storage_msg, auth = %auth_msg, "readiness probe failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "status": "unhealthy",
            "storage": probe_status_label(storage_status),
            "auth": probe_status_label(auth_status),
        })),
    )
        .into_response()
}

fn probe_log_message(status: &Result<(), RegistryError>) -> String {
    status
        .as_ref()
        .err()
        .map(RegistryError::log_message)
        .unwrap_or_else(|| "ok".to_string())
}

fn probe_status_label(status: &Result<(), RegistryError>) -> &'static str {
    if status.is_ok() { "ok" } else { "unavailable" }
}

pub(crate) async fn serve_live() -> Response {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "ok",
        })),
    )
        .into_response()
}
