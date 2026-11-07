use super::{AppState, Identity, RegistryError};

pub(in super::super) async fn require_active_account(
    state: &AppState,
    user_id: &str,
) -> Result<(), RegistryError> {
    if state.inner.identity.auth.users.account_name(user_id).await?.is_none() {
        return Err(RegistryError::Unauthenticated { resource: "a deleted account".to_string() });
    }
    Ok(())
}

pub(super) async fn account_identity(
    state: &AppState,
    user_id: &str,
) -> Result<Identity, RegistryError> {
    let username = state.inner.identity.auth.users
        .account_name(user_id)
        .await?
        .ok_or_else(|| RegistryError::Unauthenticated {
            resource: "an inactive account".to_string(),
        })?;
    Ok(Identity::account(user_id, username))
}
