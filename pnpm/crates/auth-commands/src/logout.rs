//! `pnpm logout` revokes the registry auth token on the server and
//! removes it from the global `config.yaml`, and from the `auth.ini`
//! earlier versions wrote. A token that instead lives in `.npmrc` or an
//! env var is left in place, with a warning, because pnpm owns only the
//! files it writes itself.
//!
//! Filesystem reads/writes and the token-revocation request are
//! `Sys`-bound capabilities with no `&self` receiver (production
//! provider [`Host`], test fakes are unit structs), and the two
//! `global*` log channels are emitted through the `Reporter` seam. See
//! the dependency-injection convention in `pnpm/CODE_STYLE_GUIDE.md`.

use std::{collections::HashMap, future::Future, io, path::PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::validate_json_auth_registry;
use pnpm_network::{
    RetryOpts, ThrottledClient, encode_uri_component, nerf_dart, redact_and_sanitize,
    send_with_retry,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};

use crate::{
    config_yaml::{self, GLOBAL_CONFIG_YAML_FILENAME, ParseConfigYamlError},
    ini::IniSettings,
    registry_url::normalize_registry_url,
};
use pnpm_workspace_manifest_writer::{EditManifestFieldError, ManifestEdit, edit_manifest_field};

/// The registry `pnpm logout` targets when neither `--registry` nor a
/// configured registry is given.
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// Read a file into a `String`, mirroring [`std::fs::read_to_string`].
pub trait FsReadToString {
    fn read_to_string(path: &std::path::Path) -> io::Result<String>;
}

/// Write `bytes` to a file, replacing its contents. The production [`Host`]
/// provider writes atomically, symlink-safely, and owner-only, since what is
/// written holds credentials (see [`pnpm_fs::write_atomic_private`]).
pub trait FsWrite {
    fn write(path: &std::path::Path, bytes: &[u8]) -> io::Result<()>;
}

/// Send the `DELETE -/user/token/<token>` request that revokes a token
/// on the registry. The outcome the caller acts on — accepted, rejected
/// with a status, or unreachable — is the whole contract; how the
/// request is retried and parsed is the provider's concern.
pub trait RevokeToken {
    fn revoke(
        http_client: &ThrottledClient,
        revoke_url: &str,
        token: &str,
        retry: RetryOpts,
    ) -> impl Future<Output = RevokeOutcome>;
}

/// The result of a token-revocation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeOutcome {
    /// The registry accepted the revocation (2xx).
    Revoked,
    /// The registry responded but rejected the revocation (non-2xx).
    Rejected { status: u16 },
    /// The registry could not be reached (transport error).
    Unreachable,
}

/// Production provider: real filesystem and a real `DELETE` over the
/// shared throttled HTTP client.
pub struct Host;

impl FsReadToString for Host {
    fn read_to_string(path: &std::path::Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

impl FsWrite for Host {
    fn write(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
        pnpm_fs::write_atomic_private(path, bytes)
    }
}

impl RevokeToken for Host {
    async fn revoke(
        http_client: &ThrottledClient,
        revoke_url: &str,
        token: &str,
        retry: RetryOpts,
    ) -> RevokeOutcome {
        let authorization = format!("Bearer {token}");
        // The revoke URL carries the token in its final path segment (npm's
        // `DELETE -/user/token/<token>` API). `send_with_retry` routes and
        // logs the URL it is handed, so pass the token-free prefix while the
        // request itself still targets the full `revoke_url`.
        let log_url = revoke_log_url(revoke_url);
        match send_with_retry(http_client, log_url, retry, |client| {
            client.delete(revoke_url).header(reqwest::header::AUTHORIZATION, authorization.as_str())
        })
        .await
        {
            Ok((_guard, response)) if response.status().is_success() => RevokeOutcome::Revoked,
            Ok((_guard, response)) => {
                RevokeOutcome::Rejected { status: response.status().as_u16() }
            }
            Err(_) => RevokeOutcome::Unreachable,
        }
    }
}

/// Inputs to [`logout`]. `auth_config` holds the subset of the auth
/// config the command consults: rc keys of the form
/// `//host[:port]/path/:_authToken` mapped to their raw token.
pub struct LogoutOptions<'a> {
    /// The `--registry` value; `None` falls back to [`DEFAULT_REGISTRY`].
    pub registry: Option<&'a str>,
    pub auth_config: &'a HashMap<String, String>,
    /// pnpm's `configDir`; the global config lives at
    /// `<config_dir>/config.yaml` and the legacy `auth.ini` beside it.
    pub config_dir: &'a std::path::Path,
    pub retry: RetryOpts,
    /// Reporter prefix for the `global*` log lines (the working directory).
    pub prefix: &'a str,
}

/// Log out of `registry`: revoke the token on the server, then remove it
/// from every file pnpm wrote it to. Returns the `Logged out of <registry>`
/// success line.
///
/// Errors with [`LogoutError::NotLoggedIn`] when no token is configured
/// for the registry, and [`LogoutError::LogoutFailed`] when the registry
/// rejected the revocation *and* there was no local copy to remove.
pub async fn logout<Sys, Reporter>(
    http_client: &ThrottledClient,
    opts: LogoutOptions<'_>,
) -> Result<String, LogoutError>
where
    Sys: FsReadToString + FsWrite + RevokeToken,
    Reporter: self::Reporter,
{
    let registry = normalize_registry_url(opts.registry.unwrap_or(DEFAULT_REGISTRY));
    // Canonicalized the way the reader and `pnpm login` canonicalize, so that
    // logging out names the same registry logging in did however the URL was
    // spelled on the command line. An unparsable one keeps its own spelling:
    // it matches no stored credential either way, and `NotLoggedIn` names it
    // as the user typed it.
    let registry = validate_json_auth_registry(&registry).unwrap_or_else(|_| registry.into_owned());
    let registry_config_key = nerf_dart(&registry);
    let token_key = format!("{registry_config_key}:_authToken");

    // `registry` (raw) is used for the token-key lookup and the revoke URL;
    // `registry_display` is the only form that reaches stdout, warnings, and
    // errors. A registry from an untrusted `.npmrc` / `--registry` can embed
    // `user:pass@` credentials or terminal escape sequences, so redact and
    // sanitize before it is ever shown. Mirrors `pacquet ping`.
    let registry_display = redact_and_sanitize(&registry);

    let Some(token) = opts.auth_config.get(&token_key) else {
        return Err(LogoutError::NotLoggedIn { registry: registry_display });
    };

    let revoke_url = format!("{registry}-/user/token/{}", encode_uri_component(token));
    let revoked = match Sys::revoke(http_client, &revoke_url, token, opts.retry).await {
        RevokeOutcome::Revoked => true,
        RevokeOutcome::Rejected { status } => {
            global::<Reporter>(
                opts.prefix,
                LogLevel::Info,
                format!("Registry returned HTTP {status} when revoking token"),
            );
            false
        }
        RevokeOutcome::Unreachable => {
            global::<Reporter>(
                opts.prefix,
                LogLevel::Info,
                "Could not reach the registry to revoke the token".to_string(),
            );
            false
        }
    };

    // The two files hold independent copies of the same credential, so
    // failing to clean one must not leave the other behind: a token pnpm
    // would still send is worse than a logout that reports trouble.
    let config_removal = forget_in_config_yaml::<Sys>(opts.config_dir, &registry);
    let ini_removal = forget_in_auth_ini::<Sys>(opts.config_dir, &token_key);
    let removed_from_config = config_removal?;
    let removed_from_ini = ini_removal?;

    if !removed_from_config && !removed_from_ini {
        if revoked {
            global::<Reporter>(
                opts.prefix,
                LogLevel::Warn,
                format!(
                    "The auth token for {registry_display} was not found in {}. \
                 It may be configured in .npmrc or another config file. \
                 The token was revoked on the registry but must be removed manually from that config file.",
                    opts.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME).display(),
                ),
            );
        } else {
            return Err(LogoutError::LogoutFailed {
                registry: registry_display,
                config_path: opts.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME),
            });
        }
    }

    Ok(format!("Logged out of {registry_display}"))
}

/// Drop every credential `registry` holds in the global `config.yaml`,
/// reporting whether there was one to drop.
fn forget_in_config_yaml<Sys: FsReadToString + FsWrite>(
    config_dir: &std::path::Path,
    registry: &str,
) -> Result<bool, LogoutError> {
    let path = config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
    let document = match Sys::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(LogoutError::ReadConfigYaml { path, error }),
    };

    let fields = config_yaml::logout_fields(Some(&document), registry)
        .map_err(LogoutError::ParseConfigYaml)?;
    let mut removed = false;
    let mut document = Some(document);
    for (key, value) in fields {
        let edit = edit_manifest_field(document.as_deref(), key, &value)
            .map_err(LogoutError::EditConfigYaml)?;
        let text = match edit {
            ManifestEdit::Unchanged => continue,
            // The credential was the document's only key. Blank the file
            // rather than remove it: logging out of one registry is not a
            // reason to take away a config file the user may be editing.
            ManifestEdit::Remove => String::new(),
            ManifestEdit::Write(text) => text,
        };
        Sys::write(&path, text.as_bytes())
            .map_err(|error| LogoutError::WriteConfigYaml { path: path.clone(), error })?;
        document = Some(text);
        removed = true;
    }
    Ok(removed)
}

/// Drop `token_key` from the `auth.ini` earlier versions wrote, reporting
/// whether there was one to drop.
fn forget_in_auth_ini<Sys: FsReadToString + FsWrite>(
    config_dir: &std::path::Path,
    token_key: &str,
) -> Result<bool, LogoutError> {
    let path = config_dir.join("auth.ini");
    let mut settings = safe_read_ini::<Sys>(&path)?;
    if !settings.remove(token_key) {
        return Ok(false);
    }
    Sys::write(&path, settings.serialize().as_bytes())
        .map_err(|error| LogoutError::WriteAuthIni { path, error })?;
    Ok(true)
}

/// Read `auth.ini`, treating a missing file as empty. Any other read
/// error propagates.
fn safe_read_ini<Sys: FsReadToString>(path: &std::path::Path) -> Result<IniSettings, LogoutError> {
    match Sys::read_to_string(path) {
        Ok(text) => Ok(IniSettings::parse(&text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(IniSettings::default()),
        Err(error) => Err(LogoutError::ReadAuthIni { path: path.to_path_buf(), error }),
    }
}

fn global<Reporter: self::Reporter>(prefix: &str, level: LogLevel, message: String) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog { level, message, prefix: prefix.to_string() }));
}

/// The token-free prefix of a `…/-/user/token/<token>` revoke URL — the
/// URL with its final, token-bearing path segment dropped. Handed to
/// `send_with_retry` for routing and logging so the token never reaches the
/// retry logs; the request itself still targets the full revoke URL. The
/// token is percent-encoded (so it has no literal `/`), making the last `/`
/// the segment boundary.
fn revoke_log_url(revoke_url: &str) -> &str {
    revoke_url.rsplit_once('/').map_or(revoke_url, |(prefix, _token)| prefix)
}

/// Errors surfaced by [`logout`]. The two user-facing variants carry
/// pnpm's stable error codes (`ERR_PNPM_NOT_LOGGED_IN`,
/// `ERR_PNPM_LOGOUT_FAILED`) and messages verbatim.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LogoutError {
    #[display("Not logged in to {registry}, so can't log out")]
    #[diagnostic(code(ERR_PNPM_NOT_LOGGED_IN))]
    NotLoggedIn { registry: String },

    #[display(
        "Failed to log out of {registry}. The registry rejected the token revocation request, \
         and the token was not found in {}. \
         The token may be configured in .npmrc or another config file \
         and must be removed manually, and may still need to be revoked on the registry.",
        config_path.display()
    )]
    #[diagnostic(code(ERR_PNPM_LOGOUT_FAILED))]
    LogoutFailed { registry: String, config_path: PathBuf },

    #[display("Failed to read auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_READ_AUTH_INI))]
    ReadAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_WRITE_AUTH_INI))]
    WriteAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read the global config file at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_READ_CONFIG_YAML))]
    ReadConfigYaml {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write the global config file at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_WRITE_CONFIG_YAML))]
    WriteConfigYaml {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    ParseConfigYaml(ParseConfigYamlError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    EditConfigYaml(EditManifestFieldError),
}

#[cfg(test)]
mod tests;
