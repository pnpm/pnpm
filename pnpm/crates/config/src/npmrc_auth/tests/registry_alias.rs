use super::{Config, NoEnv, NpmrcAuth, Path, assert_eq};

#[test]
fn npm_alias_normalizes_routes_and_credentials() {
    for credential_host in ["registry.npmjs.com", "registry.npmjs.org"] {
        let ini = format!(
            "registry=https://registry.npmjs.com\n@org:registry=https://registry.npmjs.com/\n//{credential_host}/:_authToken=test-token\n",
        );
        let mut config = Config::new();
        NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
        assert_eq!(config.registry, "https://registry.npmjs.org/");
        assert_eq!(config.registries_by_scope["@org"], "https://registry.npmjs.org/");
        assert_eq!(
            config.auth_headers.for_url("https://registry.npmjs.org/@org%2fpkg"),
            Some("Bearer test-token".to_owned()),
        );
        assert_eq!(config.auth_headers.for_url("https://registry.npmjs.com.evil.example/"), None);
    }
}

#[test]
fn npm_alias_normalizes_registry_tls_settings() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//registry.npmjs.com/:cert=client-certificate\n//registry.npmjs.com/:key=client-key\n",
        Path::new(""),
    );
    let tls = &auth.tls.by_uri["//registry.npmjs.org/"];
    assert_eq!(tls.cert.as_deref(), Some("client-certificate"));
    assert_eq!(tls.key.as_deref(), Some("client-key"));
    assert!(!auth.tls.by_uri.contains_key("//registry.npmjs.com/"));
}
