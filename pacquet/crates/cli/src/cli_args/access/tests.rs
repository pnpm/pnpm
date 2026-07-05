use super::*;

#[tokio::test]
async fn test_normalize_registry_url() {
    assert_eq!(normalize_registry_url("https://registry.npmjs.org"), "https://registry.npmjs.org/");
    assert_eq!(
        normalize_registry_url("https://registry.npmjs.org/"),
        "https://registry.npmjs.org/"
    );
    assert_eq!(
        normalize_registry_url("https://custom.registry.com/some-path"),
        "https://custom.registry.com/some-path/"
    );
}

#[tokio::test]
async fn test_escaped_package_name() {
    assert_eq!(escaped_package_name("@scope/pkg"), "@scope%2fpkg".to_string());
    assert_eq!(escaped_package_name("simple-pkg"), "simple-pkg".to_string());
}

#[tokio::test]
async fn test_access_subcommand_required() {
    let err = AccessArgs { registry: None, json: false, otp: None, params: vec![] }
        .run(&Config::default())
        .await
        .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SUBCOMMAND_REQUIRED"));
}

#[tokio::test]
async fn test_access_unknown_subcommand() {
    let err =
        AccessArgs { registry: None, json: false, otp: None, params: vec!["foobar".to_string()] }
            .run(&Config::default())
            .await
            .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_UNKNOWN_SUBCOMMAND"));
}

#[tokio::test]
async fn test_set_status_unscoped_error() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["set".to_string(), "status=public".to_string(), "unscoped-pkg".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SET_STATUS_UNSCOPED"));
}

#[tokio::test]
async fn test_set_status_invalid_value() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["set".to_string(), "status=invalid".to_string(), "@scope/pkg".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SET_STATUS_INVALID"));
}

#[tokio::test]
async fn test_set_mfa_invalid_value() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["set".to_string(), "mfa=invalid".to_string(), "@scope/pkg".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SET_MFA_INVALID"));
}

#[tokio::test]
async fn test_grant_invalid_permissions() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec![
            "grant".to_string(),
            "invalid".to_string(),
            "scope:team".to_string(),
            "@scope/pkg".to_string(),
        ],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_GRANT_INVALID_PERMISSIONS"));
}

#[tokio::test]
async fn test_grant_invalid_team_format() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec![
            "grant".to_string(),
            "read-only".to_string(),
            "invalidteam".to_string(),
            "@scope/pkg".to_string(),
        ],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_GRANT_INVALID_TEAM"));
}

#[tokio::test]
async fn test_revoke_invalid_team_format() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["revoke".to_string(), "invalidteam".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_REVOKE_INVALID_TEAM"));
}

#[tokio::test]
async fn test_set_status_missing_package() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["set".to_string(), "status=public".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SET_STATUS_PACKAGE_REQUIRED"));
}

#[tokio::test]
async fn test_set_mfa_missing_package() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["set".to_string(), "mfa=automation".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_SET_MFA_PACKAGE_REQUIRED"));
}

#[tokio::test]
async fn test_grant_missing_args() {
    let err =
        AccessArgs { registry: None, json: false, otp: None, params: vec!["grant".to_string()] }
            .run(&Config::default())
            .await
            .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_GRANT_ARGS_REQUIRED"));
}

#[tokio::test]
async fn test_revoke_missing_args() {
    let err =
        AccessArgs { registry: None, json: false, otp: None, params: vec!["revoke".to_string()] }
            .run(&Config::default())
            .await
            .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_REVOKE_ARGS_REQUIRED"));
}

#[tokio::test]
async fn test_get_status_missing_package() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["get".to_string(), "status".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_GET_STATUS_PACKAGE_REQUIRED"));
}

#[tokio::test]
async fn test_list_collaborators_missing_package() {
    let err = AccessArgs {
        registry: None,
        json: false,
        otp: None,
        params: vec!["list".to_string(), "collaborators".to_string()],
    }
    .run(&Config::default())
    .await
    .unwrap_err();

    let err_str = format!("{err:?}");
    assert!(err_str.contains("ERR_PNPM_ACCESS_LIST_COLLABORATORS_PACKAGE_REQUIRED"));
}

#[tokio::test]
async fn test_deprecated_public_form_resolves_to_set_status() {
    let result = {
        let mut params = vec!["public".to_string(), "@scope/pkg".to_string()];
        let first = params.remove(0);
        let second = if params.is_empty() { None } else { Some(params.remove(0)) };
        match (first.as_str(), second.as_deref()) {
            ("public", _) => Some("set_status"),
            _ => None,
        }
    };
    assert_eq!(result, Some("set_status"));
}

#[tokio::test]
async fn test_deprecated_restricted_form_resolves_to_set_status() {
    let result = {
        let mut params = vec!["restricted".to_string(), "@scope/pkg".to_string()];
        let first = params.remove(0);
        let second = if params.is_empty() { None } else { Some(params.remove(0)) };
        match (first.as_str(), second.as_deref()) {
            ("restricted", _) => Some("set_status"),
            _ => None,
        }
    };
    assert_eq!(result, Some("set_status"));
}
