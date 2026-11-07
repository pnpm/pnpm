use super::{AppState, Identity, RegistryError};

pub(in super::super) async fn require_active_account(
    state: &AppState,
    user_id: &str,
) -> Result<(), RegistryError> {
    active_account_name(state, user_id).await.map(drop)
}

pub(super) async fn active_account_name(
    state: &AppState,
    user_id: &str,
) -> Result<String, RegistryError> {
    let username = state.inner.identity.auth.users
        .account_name(user_id)
        .await?
        .ok_or_else(|| RegistryError::Unauthenticated {
            resource: "an inactive account".to_string(),
        })?;
    Ok(username)
}

pub(super) async fn account_identity(
    state: &AppState,
    user_id: &str,
) -> Result<Identity, RegistryError> {
    Ok(Identity::account(user_id, active_account_name(state, user_id).await?))
}
