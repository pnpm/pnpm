//! Proxy settings as written, merged across config layers.
//!
//! pnpm folds every source (`.npmrc` files, `pnpm-workspace.yaml`, the
//! global `config.yaml`, `PNPM_CONFIG_*`, CLI flags) into one value per
//! key and resolves the proxy cascade once over the result. A layer that
//! names a key occupies it even when the value reads as unset, so the
//! cascade falls through to *other keys* and to the environment — never
//! back to a lower-priority layer of the same key.
//!
//! [`ProxyKeys`] is that merged view: layers overwrite the keys they set,
//! and [`ProxyKeys::resolve`] turns it into the
//! [`pacquet_network::ProxyConfig`] the network layer consumes.

use crate::npmrc_auth::parse_no_proxy;
use pacquet_network::ProxyConfig;

/// One proxy key's merged value.
///
/// Which raw strings count as configured depends on the source, so each
/// layer converts through the matching constructor rather than resolution
/// re-deriving it: a scalar-typed source turns `false` and `null` into
/// non-strings, while a command-line flag carries its value verbatim.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum ProxyValue {
    /// No layer configured this key — either none named it, or one named
    /// it with a value that reads as unset. The two are indistinguishable
    /// once lower layers have been folded in, which is the point: a named
    /// key masks the layers below it whatever its value.
    #[default]
    Unset,
    Url(String),
    /// Proxying is off. Unlike [`Self::Unset`] this does not fall through
    /// to the environment. Only the legacy `proxy` key has this form —
    /// `https-proxy` and `http-proxy` are tested for truthiness, so a
    /// `false` there reads as unset.
    Disabled,
}

impl ProxyValue {
    /// A value from a scalar-typed source: an `.npmrc` or a yaml file,
    /// where `false` and `null` arrive as non-strings. Only the lowercase
    /// tokens qualify, so a capitalised `False` stays a hostname.
    #[must_use]
    pub fn from_config(raw: &str) -> Self {
        match raw {
            "" | "false" | "null" => Self::Unset,
            url => Self::Url(url.to_string()),
        }
    }

    /// [`Self::from_config`] for the legacy `proxy` key, where `false` is
    /// a value in its own right rather than an absence.
    #[must_use]
    pub fn legacy_from_config(raw: &str) -> Self {
        match raw {
            "false" => Self::Disabled,
            "" | "null" => Self::Unset,
            url => Self::Url(url.to_string()),
        }
    }

    /// A value from a command-line flag. A flag has no scalar typing, so
    /// only the empty string reads as unset and `false` is a hostname.
    #[must_use]
    pub fn from_flag(raw: &str) -> Self {
        if raw.is_empty() { Self::Unset } else { Self::Url(raw.to_string()) }
    }

    fn url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url),
            Self::Unset | Self::Disabled => None,
        }
    }
}

/// Every proxy key as written, merged across layers, plus the environment
/// fallbacks the cascade consults when no layer configures one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProxyKeys {
    pub https_proxy: ProxyValue,
    pub http_proxy: ProxyValue,
    /// The legacy `proxy` key, which feeds the HTTPS slot.
    pub legacy_proxy: ProxyValue,
    pub no_proxy: ProxyValue,
    /// The `noproxy` spelling, a separate key that [`Self::no_proxy`]
    /// falls through to.
    pub noproxy: ProxyValue,
    /// `HTTPS_PROXY`, `HTTP_PROXY`, `PROXY` and `NO_PROXY`, captured when
    /// the `.npmrc` layer is folded in so a later layer can re-resolve
    /// without reading the environment again. Environment values carry no
    /// scalar typing either, so an empty one stays an empty proxy URL that
    /// shadows the variables below it and resolves to no proxy at the
    /// client.
    pub env: ProxyEnv,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProxyEnv {
    pub https_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyKeys {
    /// Run the cascade over the merged keys.
    #[must_use]
    pub fn resolve(&self) -> ProxyConfig {
        let https = match self.https_proxy.url() {
            Some(url) => Resolved::Url(url),
            None => match &self.legacy_proxy {
                ProxyValue::Disabled => Resolved::Disabled,
                ProxyValue::Url(url) => Resolved::Url(url),
                ProxyValue::Unset => self.env.https_proxy.as_deref().into(),
            },
        };
        let http = match self.http_proxy.url() {
            Some(url) => Resolved::Url(url),
            None => match https {
                Resolved::Url(url) => Resolved::Url(url),
                Resolved::Disabled => Resolved::Disabled,
                Resolved::Unset => {
                    self.env.http_proxy.as_deref().or(self.env.proxy.as_deref()).into()
                }
            },
        };
        let no_proxy = self
            .no_proxy
            .url()
            .or_else(|| self.noproxy.url())
            .or(self.env.no_proxy.as_deref())
            .map(parse_no_proxy);
        ProxyConfig { https_proxy: https.into_url(), http_proxy: http.into_url(), no_proxy }
    }
}

/// One arm of the cascade, resolved.
///
/// [`Self::Disabled`] and [`Self::Unset`] both end as "no proxy", but only
/// `Disabled` stops the walk — see [`ProxyValue::Disabled`].
enum Resolved<'a> {
    Url(&'a str),
    Disabled,
    Unset,
}

impl Resolved<'_> {
    fn into_url(self) -> Option<String> {
        match self {
            Self::Url(url) => Some(url.to_string()),
            Self::Disabled | Self::Unset => None,
        }
    }
}

impl<'a> From<Option<&'a str>> for Resolved<'a> {
    fn from(value: Option<&'a str>) -> Self {
        value.map_or(Self::Unset, Self::Url)
    }
}
