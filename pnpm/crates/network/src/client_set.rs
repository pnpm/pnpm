//! The clients one TLS configuration is served by.

use super::{
    Arc, Client, ClientBuildInputs, ClientVariant, ForInstallsError, NetworkSettings,
    NoProxyMatcher, RedirectGuard, Resolve, TlsConfig, build_client_with_root_fallback,
};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub(super) struct ClientPair {
    pub(super) follow_redirects: Client,
    pub(super) no_redirects: Client,
}

impl ClientPair {
    pub(super) fn select(&self, follow_redirects: bool) -> &Client {
        if follow_redirects { &self.follow_redirects } else { &self.no_redirects }
    }
}

/// The clients one TLS configuration is served by, alike in everything but the
/// two choices [`ClientSet::select`] makes.
#[derive(Debug)]
pub(super) struct ClientSet {
    pub(super) decoding: ClientPair,
    /// Leave `Content-Encoding` on the response, for a caller that decodes the
    /// body itself. Built on the first archive request, so a command that
    /// downloads no archive does not pay for it.
    archive: OnceLock<ClientPair>,
    client_build: Option<ClientSetBuild>,
}

impl ClientSet {
    /// Build the decoding clients for `tls` now and the archive clients on
    /// first use.
    pub(super) fn build(
        inputs: &Arc<OwnedClientInputs>,
        tls: &TlsConfig,
    ) -> Result<Self, ForInstallsError> {
        let client_build = ClientSetBuild { inputs: Arc::clone(inputs), tls: tls.clone() };
        let decoding = client_build.build_pair(true)?;
        Ok(Self { decoding, archive: OnceLock::new(), client_build: Some(client_build) })
    }

    /// A set whose archive requests use `pair` as it is.
    pub(super) fn with_archive(decoding: ClientPair, archive: ClientPair) -> Self {
        Self { decoding, archive: OnceLock::from(archive), client_build: None }
    }

    pub(super) fn select(&self, follow_redirects: bool, decode_content: bool) -> &Client {
        let pair = if decode_content {
            &self.decoding
        } else {
            self.archive.get_or_init(|| self.build_archive())
        };
        pair.select(follow_redirects)
    }

    /// The archive clients. They differ from the decoding clients, which were
    /// built from the same inputs, only in content decoding, so a failure is a
    /// broken invariant. Falling back to the decoding clients would let
    /// reqwest decode zstd without the window bound.
    fn build_archive(&self) -> ClientPair {
        let Some(build) = &self.client_build else {
            unreachable!("a set without a client build is created with its archive clients")
        };
        build
            .build()
            .expect("the archive clients build from the inputs the decoding clients built from")
    }
}

/// What building a [`ClientSet`]'s clients needs, owned so it outlives the
/// construction of the [`super::ThrottledClient`].
pub(super) struct ClientSetBuild {
    pub(super) inputs: Arc<OwnedClientInputs>,
    pub(super) tls: TlsConfig,
}

impl std::fmt::Debug for ClientSetBuild {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ClientSetBuild").finish_non_exhaustive()
    }
}

impl ClientSetBuild {
    fn build(&self) -> Result<ClientPair, ForInstallsError> {
        self.build_pair(false)
    }

    fn build_pair(&self, decode_content: bool) -> Result<ClientPair, ForInstallsError> {
        let inputs = self.inputs.borrowed();
        let build = |forbid_redirects| {
            build_client_with_root_fallback(
                &inputs,
                &self.tls,
                ClientVariant { forbid_redirects, decode_content },
            )
        };
        Ok(ClientPair { follow_redirects: build(false)?, no_redirects: build(true)? })
    }
}

/// An owned [`ClientBuildInputs`].
pub(super) struct OwnedClientInputs {
    pub(super) settings: NetworkSettings,
    pub(super) https: Option<reqwest::Url>,
    pub(super) http: Option<reqwest::Url>,
    pub(super) no_proxy: Arc<NoProxyMatcher>,
    pub(super) extra_ca_certs: Vec<reqwest::Certificate>,
    pub(super) redirect_guard: Option<RedirectGuard>,
    pub(super) dns_resolver: Arc<dyn Resolve>,
}

impl OwnedClientInputs {
    pub(super) fn borrowed(&self) -> ClientBuildInputs<'_> {
        ClientBuildInputs {
            settings: &self.settings,
            https: self.https.clone(),
            http: self.http.clone(),
            no_proxy: Arc::clone(&self.no_proxy),
            extra_ca_certs: self.extra_ca_certs.clone(),
            redirect_guard: self.redirect_guard.as_ref(),
            dns_resolver: Arc::clone(&self.dns_resolver),
        }
    }
}
