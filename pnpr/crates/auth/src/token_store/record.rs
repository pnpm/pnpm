use super::{RegistryError, Result};

/// Supported personal token kinds.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Classic,
    Granular,
}

impl TokenKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Granular => "granular",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "classic" => Some(Self::Classic),
            "granular" => Some(Self::Granular),
            _ => None,
        }
    }

    pub(crate) fn from_sql(raw: Option<&str>, token_hash: &str) -> Result<Option<Self>> {
        match raw {
            Some(value) => Self::parse(value)
                .map(Some)
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("token {token_hash} has an unrecognized kind {value:?}"),
                }),
            None => Ok(None),
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NewToken {
    pub readonly: bool,
    pub cidr_whitelist: Vec<String>,
    pub name: Option<String>,
    /// Expiration timestamp in milliseconds since the Unix epoch.
    pub expires_at: Option<u64>,
    pub kind: Option<TokenKind>,
}

impl NewToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    #[must_use]
    pub fn with_cidr_whitelist(mut self, cidr_whitelist: Vec<String>) -> Self {
        self.cidr_whitelist = cidr_whitelist;
        self
    }

    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    #[must_use]
    pub fn with_expires_at(mut self, expires_at: Option<u64>) -> Self {
        self.expires_at = expires_at;
        self
    }

    #[must_use]
    pub fn with_kind(mut self, kind: Option<TokenKind>) -> Self {
        self.kind = kind;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the auth tokens table columns."
    )
)]
pub struct TokenRecord {
    /// Current owner name, resolved by an account join rather than stored with the token.
    pub(crate) owner_name: Option<String>,
    pub user_id: String,
    /// Creation timestamp in milliseconds since the Unix epoch.
    pub created_at: u64,
    /// Last-used timestamp in milliseconds since the Unix epoch.
    pub last_used_at: u64,
    pub readonly: bool,
    pub cidr_whitelist: Vec<String>,
    pub name: Option<String>,
    /// Expiration timestamp in milliseconds since the Unix epoch.
    pub expires_at: Option<u64>,
    pub kind: Option<TokenKind>,
}

impl TokenRecord {
    /// The current owner name when the account backend resolved it with this token.
    #[must_use]
    pub fn owner_name(&self) -> Option<&str> {
        self.owner_name.as_deref()
    }

    #[must_use]
    pub(crate) fn with_owner_name(mut self, name: String) -> Self {
        self.owner_name = Some(name);
        self
    }

    #[must_use]
    pub fn new(user_id: impl Into<String>, created_at: u64, last_used_at: u64) -> Self {
        Self {
            owner_name: None,
            user_id: user_id.into(),
            created_at,
            last_used_at,
            readonly: false,
            cidr_whitelist: Vec::new(),
            name: None,
            expires_at: None,
            kind: None,
        }
    }

    #[must_use]
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    #[must_use]
    pub fn with_cidr_whitelist(mut self, cidr_whitelist: Vec<String>) -> Self {
        self.cidr_whitelist = cidr_whitelist;
        self
    }

    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    #[must_use]
    pub fn with_expires_at(mut self, expires_at: Option<u64>) -> Self {
        self.expires_at = expires_at;
        self
    }

    #[must_use]
    pub fn with_kind(mut self, kind: Option<TokenKind>) -> Self {
        self.kind = kind;
        self
    }
}
