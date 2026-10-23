cfg_replication_or_remote_or_sync! {
    pub mod box_clone_service;
    mod http;
    pub(crate) use self::http::{ConnectorService, Socket};
}

cfg_replication! {
    pub(crate) use self::http::HttpRequestCallback;
}

cfg_replication_or_remote_or_hrana! {
    pub(crate) fn coerce_url_scheme(url: String) -> String {
        let mut url = url.replace("libsql://", "https://");

        if !url.contains("://") {
            url = format!("https://{}", url)
        }

        url
    }
}
