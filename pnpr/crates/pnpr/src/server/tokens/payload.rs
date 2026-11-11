#[cfg(test)]
mod tests;

use pnpr_error::RegistryError;
use serde::Deserialize;
use serde_json::Value;

use crate::server::parse_json_body;

const MILLIS_PER_SECOND: u64 = 1_000;
const MILLIS_PER_DAY: u64 = 86_400_000;
/// `9999-12-31T23:59:59.999Z`, the last Unix-millisecond timestamp that formats
/// as a four-digit-year RFC 3339 string.
const MAX_TOKEN_TIMESTAMP_MILLIS: u64 = 253_402_300_799_999;
const MAX_TOKEN_KIND_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenExpiry {
    Never,
    AfterMillis(u64),
    AtTimestamp(u64),
}

impl TokenExpiry {
    pub(super) fn resolve(self, now: u64) -> Result<Option<u64>, RegistryError> {
        match self {
            Self::Never => Ok(None),
            Self::AfterMillis(millis) => now
                .checked_add(millis)
                .filter(|&ts| ts <= MAX_TOKEN_TIMESTAMP_MILLIS)
                .map(Some)
                .ok_or_else(|| RegistryError::BadRequest {
                    reason: "expiry timestamp overflows supported range".to_string(),
                }),
            Self::AtTimestamp(ts) => {
                if ts <= now || ts > MAX_TOKEN_TIMESTAMP_MILLIS {
                    return Err(RegistryError::BadRequest {
                        reason: "expiry must be in the future".to_string(),
                    });
                }
                Ok(Some(ts))
            }
        }
    }
}

/// What the credential that calls the token API may pass on to a token it
/// creates without a password.
pub(super) struct CredentialLimits {
    pub(super) cidr_whitelist: Vec<String>,
    pub(super) expires_at: Option<u64>,
}

pub(super) struct ValidatedTokenRequest {
    password: Option<String>,
    access: TokenAccess,
    cidr_whitelist: Vec<String>,
    name: Option<String>,
    expiry: TokenExpiry,
    latest_expiry: Option<u64>,
    kind: Option<String>,
}

impl ValidatedTokenRequest {
    pub(super) fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Narrow the request to `limits`. Without a requested CIDR whitelist the
    /// token takes the caller's. A requested network outside the caller's
    /// whitelist is refused. The token expires no later than the caller's
    /// credential.
    pub(super) fn within(mut self, limits: CredentialLimits) -> Result<Self, RegistryError> {
        if !limits.cidr_whitelist.is_empty() {
            if self.cidr_whitelist.is_empty() {
                self.cidr_whitelist = limits.cidr_whitelist;
            } else {
                check_cidr_whitelist_within(&self.cidr_whitelist, &limits.cidr_whitelist)?;
            }
        }
        self.latest_expiry = limits.expires_at;
        Ok(self)
    }

    pub(super) fn into_new_token(self, now: u64) -> Result<pnpr_auth::NewToken, RegistryError> {
        let requested_expiry = self.expiry.resolve(now)?;
        let expires_at = match (requested_expiry, self.latest_expiry) {
            (Some(requested), Some(latest)) => Some(requested.min(latest)),
            (requested, latest) => requested.or(latest),
        };
        let readonly = match self.access {
            TokenAccess::ReadOnly => true,
            TokenAccess::ReadWrite => false,
        };
        Ok(pnpr_auth::NewToken::new()
            .with_readonly(readonly)
            .with_cidr_whitelist(self.cidr_whitelist)
            .with_name(self.name)
            .with_expires_at(expires_at)
            .with_kind(self.kind))
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "npm POST /-/npm/v1/tokens request body format"
    )
)]
struct CreateTokenPayload {
    password: Option<String>,
    readonly: Option<bool>,
    cidr_whitelist: Option<Vec<String>>,
    name: Option<String>,
    #[serde(rename = "description")]
    _description: Option<String>,
    expires: Option<Value>,
    expires_in: Option<u64>,
    kind: Option<String>,
    packages_and_scopes_permission: Option<String>,
    orgs_permission: Option<String>,
    packages_all: Option<bool>,
    #[serde(rename = "bypass_2fa")]
    _bypass_2fa: Option<bool>,
    packages: Option<Value>,
    scopes: Option<Value>,
    orgs: Option<Value>,
}

pub(super) fn parse_and_validate_token_request(
    body: &[u8],
    now: u64,
) -> Result<ValidatedTokenRequest, RegistryError> {
    let payload =
        if body.is_empty() { CreateTokenPayload::default() } else { parse_json_body(body)? };
    reject_unsupported_restrictions(&payload)?;
    let access =
        resolve_token_access(payload.readonly, payload.packages_and_scopes_permission.as_deref())?;
    let cidr_whitelist = payload.cidr_whitelist.unwrap_or_default();
    validate_cidr_whitelist(&cidr_whitelist)?;
    validate_kind(payload.kind.as_deref())?;
    let expiry = parse_token_expiry(payload.expires.as_ref(), payload.expires_in)?;
    expiry.resolve(now)?;
    Ok(ValidatedTokenRequest {
        password: payload.password,
        access,
        cidr_whitelist,
        name: payload.name,
        expiry,
        latest_expiry: None,
        kind: payload.kind,
    })
}

fn reject_unsupported_restrictions(payload: &CreateTokenPayload) -> Result<(), RegistryError> {
    if payload.packages.is_some() {
        return Err(RegistryError::BadRequest {
            reason: "package-scoped tokens are not supported; omit 'packages'".to_string(),
        });
    }
    if payload.scopes.is_some() {
        return Err(RegistryError::BadRequest {
            reason: "scope-restricted tokens are not supported; omit 'scopes'".to_string(),
        });
    }
    if payload.orgs.is_some() || payload.orgs_permission.is_some() {
        return Err(RegistryError::BadRequest {
            reason: "organization-scoped token permissions are not supported; omit 'orgs' and 'orgs_permission'".to_string(),
        });
    }
    if payload.packages_all == Some(false) {
        return Err(RegistryError::BadRequest {
            reason: "packages_all cannot be false".to_string(),
        });
    }
    Ok(())
}

fn resolve_token_access(
    readonly: Option<bool>,
    permission: Option<&str>,
) -> Result<TokenAccess, RegistryError> {
    let parsed_permission = permission.map(parse_packages_permission).transpose()?;
    match (readonly, parsed_permission) {
        (Some(true), None | Some(TokenAccess::ReadOnly)) | (None, Some(TokenAccess::ReadOnly)) => {
            Ok(TokenAccess::ReadOnly)
        }
        (Some(false) | None, None | Some(TokenAccess::ReadWrite)) => Ok(TokenAccess::ReadWrite),
        (Some(true), Some(TokenAccess::ReadWrite)) | (Some(false), Some(TokenAccess::ReadOnly)) => {
            Err(RegistryError::BadRequest {
                reason: "conflicting 'readonly' and 'packages_and_scopes_permission' values"
                    .to_string(),
            })
        }
    }
}

fn parse_packages_permission(raw: &str) -> Result<TokenAccess, RegistryError> {
    match raw {
        "read-only" => Ok(TokenAccess::ReadOnly),
        "read-write" => Ok(TokenAccess::ReadWrite),
        "read-write-stage-only" | "no-access" => Err(RegistryError::BadRequest {
            reason: format!("packages_and_scopes_permission '{raw}' is not supported"),
        }),
        other => Err(RegistryError::BadRequest {
            reason: format!("invalid packages_and_scopes_permission: {other}"),
        }),
    }
}

fn validate_cidr_whitelist(whitelist: &[String]) -> Result<(), RegistryError> {
    for entry in whitelist {
        if pnpr_config::IpNetwork::parse(entry.trim()).is_err() {
            return Err(RegistryError::BadRequest {
                reason: format!("invalid CIDR whitelist entry: {entry}"),
            });
        }
    }
    Ok(())
}

fn check_cidr_whitelist_within(
    requested: &[String],
    allowed: &[String],
) -> Result<(), RegistryError> {
    let allowed = pnpr_config::IpNetworkSet::new(
        allowed
            .iter()
            .filter_map(|entry| pnpr_config::IpNetwork::parse(entry.trim()).ok()),
    );
    for entry in requested {
        let inside = pnpr_config::IpNetwork::parse(entry.trim())
            .is_ok_and(|network| allowed.covers(&network));
        if !inside {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "CIDR whitelist entry {entry} is outside the calling token's CIDR whitelist",
                ),
            });
        }
    }
    Ok(())
}

fn validate_kind(kind: Option<&str>) -> Result<(), RegistryError> {
    if let Some(kind) = kind
        && kind.chars().count() > MAX_TOKEN_KIND_CHARS
    {
        return Err(RegistryError::BadRequest {
            reason: format!("kind must be at most {MAX_TOKEN_KIND_CHARS} characters"),
        });
    }
    Ok(())
}

fn parse_token_expiry(
    expires: Option<&Value>,
    expires_in: Option<u64>,
) -> Result<TokenExpiry, RegistryError> {
    match (expires, expires_in) {
        (Some(_), Some(_)) => Err(RegistryError::BadRequest {
            reason: "cannot specify both 'expires' and 'expires_in'".to_string(),
        }),
        (None, Some(0)) => Err(RegistryError::BadRequest {
            reason: "expires_in must be greater than 0".to_string(),
        }),
        (None, Some(seconds)) => expiry_from_seconds(seconds),
        (Some(val), None) => parse_expires_value(val),
        (None, None) => Ok(TokenExpiry::Never),
    }
}

fn parse_expires_value(val: &Value) -> Result<TokenExpiry, RegistryError> {
    match val {
        Value::Number(num) => {
            let days = num
                .as_u64()
                .ok_or_else(|| RegistryError::BadRequest {
                    reason:
                        "expires must be a positive integer number of days or an RFC 3339 timestamp"
                            .to_string(),
                })?;
            expiry_from_days(days)
        }
        Value::String(raw) => parse_expires_string(raw),
        Value::Null => Ok(TokenExpiry::Never),
        _ => Err(RegistryError::BadRequest { reason: "invalid expires value".to_string() }),
    }
}

fn parse_expires_string(raw: &str) -> Result<TokenExpiry, RegistryError> {
    if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        let days = raw
            .parse::<u64>()
            .map_err(|_| RegistryError::BadRequest {
                reason: "expiry timestamp overflows supported range".to_string(),
            })?;
        return expiry_from_days(days);
    }
    let dt = chrono::DateTime::parse_from_rfc3339(raw)
        .map_err(|_| RegistryError::BadRequest {
            reason: format!("unsupported expires format: {raw}"),
        })?;
    let Ok(timestamp) = u64::try_from(dt.timestamp_millis()) else {
        return Err(RegistryError::BadRequest {
            reason: "expiry must be in the future".to_string(),
        });
    };
    Ok(TokenExpiry::AtTimestamp(timestamp))
}

fn expiry_from_seconds(seconds: u64) -> Result<TokenExpiry, RegistryError> {
    let millis = seconds
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: "expiry timestamp overflows supported range".to_string(),
        })?;
    Ok(TokenExpiry::AfterMillis(millis))
}

fn expiry_from_days(days: u64) -> Result<TokenExpiry, RegistryError> {
    if days == 0 {
        return Err(RegistryError::BadRequest {
            reason: "expires must be greater than 0 days".to_string(),
        });
    }
    let millis = days
        .checked_mul(MILLIS_PER_DAY)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: "expiry timestamp overflows supported range".to_string(),
        })?;
    Ok(TokenExpiry::AfterMillis(millis))
}
