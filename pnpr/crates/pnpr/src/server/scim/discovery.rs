//! The SCIM discovery endpoints identity providers read before they
//! provision: what pnpr supports, and the one resource type it serves.

use super::{ScimClient, StatusCode, json, resources::USER_SCHEMA, scim_json};
use axum::response::Response;

/// `GET /ServiceProviderConfig`.
pub(super) async fn service_provider_config(_client: ScimClient) -> Response {
    let unsupported = json!({ "supported": false });
    scim_json(
        StatusCode::OK,
        &json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "patch": { "supported": true },
            "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
            "filter": { "supported": true, "maxResults": 1000 },
            "changePassword": unsupported,
            "sort": unsupported,
            "etag": unsupported,
            "authenticationSchemes": [{
                "type": "oauthbearertoken",
                "name": "Bearer token",
                "description": "The auth.scim.token of the pnpr configuration",
            }],
        }),
    )
}

/// `GET /ResourceTypes`.
pub(super) async fn resource_types(_client: ScimClient) -> Response {
    scim_json(
        StatusCode::OK,
        &json!([{
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
            "id": "User",
            "name": "User",
            "endpoint": "/Users",
            "schema": USER_SCHEMA,
        }]),
    )
}

/// `GET /Schemas`: the `User` attributes pnpr reads.
pub(super) async fn schemas(_client: ScimClient) -> Response {
    let attribute = |name: &str, kind: &str, required: bool| {
        json!({
            "name": name,
            "type": kind,
            "multiValued": false,
            "required": required,
            "mutability": "readWrite",
            "returned": "default",
            "uniqueness": if name == "userName" { "server" } else { "none" },
        })
    };
    scim_json(
        StatusCode::OK,
        &json!([{
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
            "id": USER_SCHEMA,
            "name": "User",
            "attributes": [attribute("userName", "string", true), attribute("active", "boolean", false)],
        }]),
    )
}
