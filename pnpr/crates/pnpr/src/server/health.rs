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

pub(crate) async fn check_server_readiness(state: &AppState) -> Result<(), RegistryError> {
    state.inner.storage.check_readiness().await?;
    state.inner.identity.auth.check_readiness().await?;
    Ok(())
}

pub(crate) async fn serve_ping(State(state): State<AppState>) -> Response {
    match check_server_readiness(&state).await {
        Ok(()) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
        Err(err) => {
            tracing::warn!(error = %err.log_message(), "ping readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, err.public_message()).into_response()
        }
    }
}

pub(crate) async fn serve_ready(State(state): State<AppState>) -> Response {
    let storage_status = state.inner.storage.check_readiness().await;
    let auth_status = state.inner.identity.auth.check_readiness().await;

    if storage_status.is_ok() && auth_status.is_ok() {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "status": "ok",
                "storage": "ok",
                "auth": "ok",
            })),
        )
            .into_response()
    } else {
        let storage_msg = storage_status
            .as_ref()
            .err()
            .map(RegistryError::log_message)
            .unwrap_or_else(|| "ok".to_string());
        let auth_msg = auth_status
            .as_ref()
            .err()
            .map(RegistryError::log_message)
            .unwrap_or_else(|| "ok".to_string());
        tracing::warn!(storage = %storage_msg, auth = %auth_msg, "readiness probe failed");
        let storage_status_val = if storage_status.is_ok() { "ok" } else { "unavailable" };
        let auth_status_val = if auth_status.is_ok() { "ok" } else { "unavailable" };
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "status": "unhealthy",
                "storage": storage_status_val,
                "auth": auth_status_val,
            })),
        )
            .into_response()
    }
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
