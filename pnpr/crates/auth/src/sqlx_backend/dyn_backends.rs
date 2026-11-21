use super::{
    super::{
        AccountCredential,
        AccountStatus,
        NewToken,
        PasswordChange,
        TokenBackend,
        TokenRecord,
        UpsertOutcome,
        UserBackend,
        UserCreation,
        UserRemoval,
    },
    Result,
    SqlAuth,
};
use async_trait::async_trait;

// `#[async_trait]` boxes every method's future as `Send`. Rust cannot prove
// that for the `async fn` futures of a generic `Db: AuthSqlBackend`, but it
// can once `Db` is concrete, so the `UserBackend` and `TokenBackend` impls
// are stamped per database rather than written once over `Db`.
macro_rules! impl_dyn_backends {
    ($db:ty) => {
        #[async_trait]
        impl UserBackend for SqlAuth<$db> {
            async fn add_or_login(
                &self,
                username: &str,
                password: &str,
            ) -> Result<(UpsertOutcome, String)> {
                self.add_or_login(username, password).await
            }

            async fn list_users(&self) -> Result<Vec<String>> {
                self.list_users().await
            }

            async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
                self.list_accounts().await
            }

            async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<crate::UserAccount>> {
                self.accounts_by_id(user_ids).await
            }

            async fn credential(&self, username: &str) -> Result<Option<AccountCredential>> {
                self.credential(username).await
            }

            async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
                self.create_user(username, password).await
            }

            async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
                self.set_password(user_id, password).await
            }

            async fn set_password_if_current(
                &self,
                user_id: &str,
                password: &str,
                expected_hash: &str,
            ) -> Result<bool> {
                self.set_password_if_current(user_id, password, expected_hash).await
            }

            async fn clear_password(&self, user_id: &str) -> Result<PasswordChange> {
                self.clear_password(user_id).await
            }

            async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
                self.delete_user(user_id).await
            }

            async fn provision_user(&self, username: &str) -> Result<String> {
                self.provision_user(username).await
            }

            async fn account_id(&self, username: &str) -> Result<Option<String>> {
                self.account_id(username).await
            }

            async fn active_account_id(&self, username: &str) -> Result<Option<String>> {
                self.active_account_id(username).await
            }

            async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
                self.account_name(user_id).await
            }

            async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
                self.active_account_name(user_id).await
            }

            async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
                self.account_status(user_id).await
            }

            async fn status_revisions(
                &self,
                user_ids: &[String],
            ) -> Result<std::collections::HashMap<String, crate::AccountStatusRevision>> {
                self.status_revisions(user_ids).await
            }

            async fn apply_account_status(
                &self,
                user_id: &str,
                revision: u64,
                lifecycle: crate::AccountLifecycle,
            ) -> Result<crate::AccountStatusOutcome> {
                self.apply_account_status(user_id, revision, lifecycle).await
            }

            fn revokes_tokens_in_status_transition(&self) -> bool {
                true
            }

            async fn account_login(&self, user_id: &str) -> Result<Option<crate::AccountLogin>> {
                self.account_login(user_id).await
            }

            async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
                self.rename_user(user_id, username).await
            }

            async fn rename_user_if_current(
                &self,
                user_id: &str,
                expected_revision: u64,
                username: &str,
            ) -> Result<crate::UserRename> {
                self.rename_user_if_current(user_id, expected_revision, username).await
            }

            async fn check_readiness(&self) -> Result<()> {
                self.check_db_readiness().await
            }
        }

        #[async_trait]
        impl TokenBackend for SqlAuth<$db> {
            async fn check_readiness(&self) -> Result<()> {
                self.check_db_readiness().await
            }

            async fn issue_token(
                &self,
                user_id: &str,
                new_token: NewToken,
            ) -> Result<(String, TokenRecord)> {
                self.issue_token(user_id, new_token).await
            }

            async fn issue(&self, user_id: &str) -> Result<String> {
                self.issue(user_id).await
            }

            async fn lookup(&self, raw: &str) -> Result<Option<String>> {
                self.lookup(raw).await
            }

            async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
                self.lookup_record(raw).await
            }

            async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
                self.touch_key(key, now).await
            }

            async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
                self.find_by_key(key).await
            }

            async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
                self.list_for_user(user_id).await
            }

            async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
                self.revoke_by_key(key).await
            }

            async fn revoke_all_for_user(&self, user_id: &str) -> Result<usize> {
                self.revoke_all_for_user(user_id).await
            }

            async fn delete_expired(&self, now: u64) -> Result<usize> {
                self.delete_expired(now).await
            }
        }
    };
}

#[cfg(feature = "backend-postgres")]
impl_dyn_backends!(super::postgres::PostgresDatabase);

#[cfg(feature = "backend-mysql")]
impl_dyn_backends!(super::mysql::MysqlDatabase);
