//! The authorization-code client a login runs on, built from the provider's
//! discovered metadata and the client-secret method it advertises.

use super::{
    AuthType, ClientId, ClientSecret, CoreClient, CoreClientAuthMethod, CoreProviderMetadata,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, OidcState, Provider, RedirectUrl, Result,
    invalid_config, rejected,
};

pub(super) type LoginClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

impl OidcState {
    pub(super) fn login_client(
        &self,
        provider: &Provider,
        metadata: CoreProviderMetadata,
    ) -> Result<LoginClient> {
        let config = &provider.config;
        let login = config.login.as_ref().ok_or_else(rejected)?;
        let auth_type = if login.client_secret.is_none() {
            AuthType::RequestBody
        } else if metadata
            .token_endpoint_auth_methods_supported()
            .is_none_or(|methods| methods.contains(&CoreClientAuthMethod::ClientSecretBasic))
        {
            AuthType::BasicAuth
        } else if metadata
            .token_endpoint_auth_methods_supported()
            .is_some_and(|methods| methods.contains(&CoreClientAuthMethod::ClientSecretPost))
        {
            AuthType::RequestBody
        } else {
            return Err(invalid_config(
                "OIDC provider does not support client secret authentication",
            ));
        };
        Ok(CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.audience.clone()),
            login.client_secret.clone().map(ClientSecret::new),
        )
        .set_auth_type(auth_type)
        .set_redirect_uri(
            RedirectUrl::new(format!("{}/-/oidc/{}/callback", self.public_url, config.name))
                .map_err(|_| invalid_config("invalid OIDC callback URL"))?,
        ))
    }
}
