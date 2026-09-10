use super::{
    AccessContext, AccessError, Context, IntoDiagnostic, Method, StatusCode, encode_uri_component,
    escaped_package_name, fetch_error_from_response, normalize_registry_url, send_get, send_json,
    write_error_from_response,
};

pub(super) async fn get_status(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::GetStatusPackageRequired)?;

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let (_guard, response) = send_get(context, &url, auth_header.as_deref())
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry access status endpoint")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(AccessError::PackageNotFound { package_name: package_name.clone() }.into());
    }
    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "get status of").await);
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct AccessStatus {
        access: Option<String>,
        #[serde(rename = "publish_requires_tfa")]
        publish_requires_tfa: Option<serde_json::Value>,
    }

    let status: AccessStatus =
        response.json().await.into_diagnostic().wrap_err("parsing the access status response")?;

    if context.json {
        let output = serde_json::to_string_pretty(&status)
            .into_diagnostic()
            .wrap_err("serializing access status to JSON")?;
        return Ok(output);
    }

    let access = status.access.as_deref().unwrap_or("public");
    Ok(format!("package: {package_name}\naccess: {access}"))
}

pub(super) async fn set_status(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let status_val = params
        .first()
        .ok_or(AccessError::SetStatusRequired)?
        .strip_prefix("status=")
        .ok_or(AccessError::SetStatusRequired)?;

    let access_value = match status_val {
        "public" => "public",
        "private" | "restricted" => "restricted",
        other => return Err(AccessError::SetStatusInvalid { value: other.to_string() }.into()),
    };

    let package_name = params.get(1).ok_or(AccessError::SetStatusPackageRequired)?;

    if !package_name.starts_with('@') {
        return Err(AccessError::SetStatusUnscoped.into());
    }

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let body = serde_json::json!({ "access": access_value });

    let (_guard, response) = send_json(context, Method::POST, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry access set endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!(r#"set access to "{access_value}" for"#),
            package_name,
        )
        .await);
    }

    let display_access = if access_value == "restricted" { "restricted" } else { "public" };
    Ok(format!("{package_name}: {display_access}"))
}

pub(super) async fn set_mfa(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let mfa_val = params
        .first()
        .ok_or(AccessError::SetMfaRequired)?
        .strip_prefix("mfa=")
        .ok_or(AccessError::SetMfaRequired)?;

    let publish_requires_tfa = match mfa_val {
        "none" => false,
        "publish" | "automation" => true,
        other => return Err(AccessError::SetMfaInvalid { value: other.to_string() }.into()),
    };

    let package_name = params.get(1).ok_or(AccessError::SetMfaPackageRequired)?;

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let body = serde_json::json!({ "publish_requires_tfa": publish_requires_tfa });

    let (_guard, response) = send_json(context, Method::POST, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry MFA set endpoint")?;

    if !response.status().is_success() {
        return Err(
            write_error_from_response(response, "set MFA for".to_string(), package_name).await
        );
    }

    Ok(format!("{package_name}: mfa={mfa_val}"))
}

pub(super) async fn grant_access(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let (permissions, scope_team, package_name) = grant_parameters(params)?;

    let parts: Vec<&str> = scope_team.splitn(2, ':').collect();
    let scope = parts[0].strip_prefix('@').unwrap_or(parts[0]);
    let team = parts[1];

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/team/{}/{}/package",
        normalize_registry_url(&context.registry),
        encode_uri_component(scope),
        encode_uri_component(team),
    );

    let body = serde_json::json!({
        "package": package_name,
        "permissions": permissions,
    });

    let (_guard, response) = send_json(context, Method::PUT, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry grant access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("grant {permissions} access for {scope_team} on"),
            package_name,
        )
        .await);
    }

    Ok(format!("+{scope_team} ({permissions}): {package_name}"))
}

pub(super) async fn revoke_access(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    if params.is_empty() {
        return Err(AccessError::RevokeArgsRequired.into());
    }

    let scope_team = &params[0];
    if !scope_team.contains(':') {
        return Err(AccessError::RevokeInvalidTeam { team: scope_team.clone() }.into());
    }

    let package_name = params.get(1).ok_or(AccessError::RevokePackageRequired)?;

    let parts: Vec<&str> = scope_team.splitn(2, ':').collect();
    let scope = parts[0].strip_prefix('@').unwrap_or(parts[0]);
    let team = parts[1];

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/team/{}/{}/package",
        normalize_registry_url(&context.registry),
        encode_uri_component(scope),
        encode_uri_component(team),
    );

    let body = serde_json::json!({ "package": package_name });

    let (_guard, response) =
        send_json(context, Method::DELETE, &url, auth_header.as_deref(), &body)
            .await
            .map_err(reqwest::Error::without_url)
            .into_diagnostic()
            .wrap_err("requesting the registry revoke access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("revoke {scope_team}'s access to"),
            package_name,
        )
        .await);
    }

    Ok(format!("-{scope_team}: {package_name}"))
}

fn grant_parameters(params: &[String]) -> miette::Result<(&str, &str, &str)> {
    if params.len() < 2 {
        return Err(AccessError::GrantArgsRequired.into());
    }

    let permissions = &params[0];
    if permissions != "read-only" && permissions != "read-write" {
        return Err(AccessError::GrantInvalidPermissions { value: permissions.clone() }.into());
    }

    let scope_team = &params[1];
    if !scope_team.contains(':') {
        return Err(AccessError::GrantInvalidTeam { team: scope_team.clone() }.into());
    }

    let package_name = params.get(2).ok_or(AccessError::GrantPackageRequired)?;

    Ok((permissions, scope_team, package_name))
}
