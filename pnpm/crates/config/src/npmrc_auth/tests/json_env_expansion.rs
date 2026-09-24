use super::{EnvVar, NpmrcAuth, assert_eq, default_auth_token, scoped_auth_token};

#[test]
fn trusted_json_auth_expands_only_tokens_after_source_precedence() {
    static_env!(
        Env,
        &[
            ("TOKEN", "test-token"),
            ("ORG_TOKEN", "org-token"),
            ("pnpm_config__auth", r#"{"https://registry.example":{"@":{"authToken":"${TOKEN}"}}}"#),
        ]
    );
    let global = serde_json::json!({
        "https://registry.example": {
            "@": { "authToken": "${OVERRIDDEN_MISSING}" },
            "@org": { "authToken": "${ORG_TOKEN}" },
        },
        "https://other.example": { "@": { "authToken": "${TOKEN}" } },
    });
    let auth = NpmrcAuth::from_json_sources::<Env>(Some(&global)).expect("valid auth");
    assert_eq!(default_auth_token(&auth, "//registry.example/"), Some(Some("test-token")));
    assert_eq!(scoped_auth_token(&auth, "//registry.example/", "@org"), Some(Some("org-token")));
    assert_eq!(default_auth_token(&auth, "//other.example/"), Some(Some("test-token")));
    assert_eq!(
        auth.routes.json_env.get("default").map(String::as_str),
        Some("https://registry.example/")
    );
    assert_eq!(
        auth.routes.json_file.get("@org").map(String::as_str),
        Some("https://registry.example/")
    );
    assert_eq!(auth.warnings, Vec::<String>::new());
}

#[test]
fn trusted_json_auth_uses_existing_placeholder_semantics_without_leaking_tokens() {
    static_env!(Env, &[("TOKEN", "test-secret"), ("EMPTY", "")]);
    for (token, expected, warnings) in [
        ("${TOKEN}", "test-secret", vec![]),
        ("${MISSING:-fallback}", "fallback", vec![]),
        ("${MISSING-fallback}", "fallback", vec![]),
        ("${EMPTY-fallback}", "", vec![]),
        ("${EMPTY:-fallback}", "fallback", vec![]),
        (r"\${TOKEN}", "${TOKEN}", vec![]),
        ("${MISSING}", "", vec!["Failed to replace env in config: ${MISSING} in _auth.authToken"]),
        ("${EMPTY}", "", vec!["Failed to replace env in config: ${EMPTY} in _auth.authToken"]),
        (
            "${TOKEN}${MISSING}",
            "test-secret",
            vec!["Failed to replace env in config: ${MISSING} in _auth.authToken"],
        ),
    ] {
        let global = serde_json::json!({
            "https://registry.example": { "@": { "authToken": token } },
        });
        let auth = NpmrcAuth::from_json_sources::<Env>(Some(&global)).expect("valid auth");
        assert_eq!(default_auth_token(&auth, "//registry.example/"), Some(Some(expected)));
        assert_eq!(auth.warnings, warnings);
    }
}
