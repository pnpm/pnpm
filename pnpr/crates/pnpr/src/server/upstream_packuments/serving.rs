use axum::{
    body::Bytes,
    response::IntoResponse,
};

use super::{
    AppState,
    CanonicalPackageName,
    DistView,
    DocumentSource,
    HeaderMap,
    Identity,
    PackumentRead,
    RegistryError,
    Response,
    Upstream,
    authorized_upstream,
    fetch_coalesced_upstream_packument,
    not_found,
    packument_response,
    read_cached_upstream_packument,
    upstream_cache_namespace,
};

pub(in crate::server) async fn serve_packument_via_upstream(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    upstream: &str,
    name: &CanonicalPackageName,
    tarball_base: &str,
    view: DistView<'_>,
) -> Response {
    let client = match authorized_upstream(state, identity, upstream) {
        Ok(client) => client,
        Err(err) => return err.into_response(),
    };
    let revalidate = headers
        .get_all(axum::http::header::CACHE_CONTROL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|directive| directive.trim().eq_ignore_ascii_case("no-cache"));
    let ttl = client.packument_ttl().unwrap_or(state.inner.config.http.packument_ttl);
    let document = UpstreamDocument {
        state,
        upstream: &client,
        namespace: upstream_cache_namespace(state, upstream),
        name,
        read: if revalidate { PackumentRead::Revalidate } else { PackumentRead::Cached(ttl) },
        response: DocumentResponse { headers, tarball_base, view },
    };
    match document.cached().await {
        Ok(Some(response)) => response,
        Ok(None) => document.fetched().await,
        Err(err) => err.into_response(),
    }
}

struct UpstreamDocument<'a> {
    state: &'a AppState,
    upstream: &'a Upstream,
    namespace: String,
    name: &'a CanonicalPackageName,
    read: PackumentRead,
    response: DocumentResponse<'a>,
}

struct DocumentResponse<'a> {
    headers: &'a HeaderMap,
    tarball_base: &'a str,
    view: DistView<'a>,
}

impl UpstreamDocument<'_> {
    async fn cached(&self) -> Result<Option<Response>, RegistryError> {
        let Some(ttl) = self.read.ttl() else {
            return Ok(None);
        };
        if !self.upstream.caches() {
            return Ok(None);
        }
        let response = async {
            let Some(bytes) =
                read_cached_upstream_packument(self.state, &self.namespace, self.name, ttl).await?
            else {
                return Ok(None);
            };
            self.render(bytes.into()).await.map(Some)
        };
        self.state.inner.packuments.admissions.serve(DocumentSource::Stored, response).await
    }

    async fn fetched(&self) -> Response {
        let document = match fetch_coalesced_upstream_packument(
            self.state,
            &self.namespace,
            self.upstream,
            self.name,
            self.read,
        )
        .await
        {
            Ok(Some(document)) => document,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let rendering = self.render(document.bytes.clone());
        let response =
            self.state.inner.packuments.admissions.serve(DocumentSource::Stored, rendering).await;
        drop(document);
        match response {
            Ok(response) => response,
            Err(err) => err.into_response(),
        }
    }

    async fn render(&self, bytes: Bytes) -> Result<Response, RegistryError> {
        let table = self.state.inner.storage.read_tar_integrities(self.name).await?;
        let view = DistView { tar_integrities: Some(&table), ..self.response.view };
        packument_response(
            self.name,
            bytes,
            self.response.tarball_base,
            view,
            self.state,
            self.response.headers,
            self.upstream
                .response_ttl()
                .unwrap_or(self.state.inner.config.http.packument_ttl)
                .as_secs(),
        )
        .await
    }
}
