pub use error::ProfileError;

mod error;
mod registry;
mod render;
#[cfg(test)]
mod tests;

use crate::cli_args::{registry_client::build_registry_client_with_otp_guard, sanitize::sanitize};
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pnpm_auth_commands::login::{Host, PromptPassword};
use pnpm_config::Config;
use pnpm_network::normalize_registry_url;
use pnpm_network_web_auth::{PromptOtp, generate_qr_code};
use registry::ProfileRegistry;
use render::{format_profile_map, render_all_properties, render_selected_properties, tfa_mode};
use serde_json::{Map, Value, json};

const WRITABLE_PROFILE_KEYS: &[&str] =
    &["email", "password", "fullname", "homepage", "freenode", "twitter", "github"];

const TFA_MODES: &[&str] = &["auth-only", "auth-and-writes"];

#[derive(Debug, Args)]
pub struct ProfileArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Output results as JSON.
    #[clap(long)]
    pub json: bool,

    /// Output results in tab-separated format.
    #[clap(short, long)]
    pub parseable: bool,

    /// `get [<key>...]`, `set <key> <value>`, `set password`,
    /// `enable-2fa [auth-only|auth-and-writes]`, or `disable-2fa`.
    pub params: Vec<String>,
}

/// A validated `pnpm profile` invocation.
#[derive(Debug, PartialEq, Eq)]
enum ProfileCommand {
    Get {
        properties: Vec<String>,
    },
    /// `value` is `None` for `password`, whose values are prompted for.
    Set {
        property: String,
        value: Option<String>,
    },
    EnableTfa {
        mode: String,
    },
    DisableTfa,
}

impl ProfileArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<Option<String>> {
        self.execute::<Host>(config).await
    }

    async fn execute<Sys>(&self, config: &Config) -> miette::Result<Option<String>>
    where
        Sys: PromptPassword + PromptOtp + 'static,
    {
        let command = self.parse_command()?;
        let registry_url =
            normalize_registry_url(self.registry.as_deref().unwrap_or(&config.registry));
        let auth_header =
            config.auth_headers.for_url(&registry_url).ok_or(ProfileError::Unauthorized)?;
        let client = build_registry_client_with_otp_guard(
            config,
            self.otp.as_deref(),
            [registry_url.as_ref()],
        )?;
        let registry = ProfileRegistry {
            url: &registry_url,
            client: &client,
            auth_header: &auth_header,
            otp: self.otp.as_deref(),
            retry_opts: config.retry_opts(),
        };
        match command {
            ProfileCommand::Get { properties } => self.get(&registry, &properties).await,
            ProfileCommand::Set { property, value } => {
                self.set::<Sys>(&registry, &property, value).await
            }
            ProfileCommand::EnableTfa { mode } => enable_tfa::<Sys>(&registry, &mode).await,
            ProfileCommand::DisableTfa => self.disable_tfa::<Sys>(&registry).await,
        }
    }

    fn parse_command(&self) -> Result<ProfileCommand, ProfileError> {
        let Some((subcommand, rest)) = self.params.split_first() else {
            return Err(ProfileError::SubcommandRequired);
        };
        match subcommand.as_str() {
            "get" => Ok(ProfileCommand::Get { properties: split_property_list(rest) }),
            "set" => parse_set(rest),
            "enable-2fa" | "enable-tfa" | "enable2fa" | "enabletfa" => self.parse_enable_tfa(rest),
            "disable-2fa" | "disable-tfa" | "disable2fa" | "disabletfa" => {
                Ok(ProfileCommand::DisableTfa)
            }
            other => Err(ProfileError::UnknownCommand { command: other.to_string() }),
        }
    }

    fn parse_enable_tfa(&self, rest: &[String]) -> Result<ProfileCommand, ProfileError> {
        let mode = match rest {
            [] => "auth-and-writes",
            [mode] => mode.as_str(),
            _ => return Err(ProfileError::EnableTfaArgs),
        };
        if !TFA_MODES.contains(&mode) {
            return Err(ProfileError::Invalid2faMode { mode: mode.to_string() });
        }
        if self.json {
            return Err(ProfileError::TfaInteractive { format: "JSON" });
        }
        if self.parseable {
            return Err(ProfileError::TfaInteractive { format: "parseable" });
        }
        Ok(ProfileCommand::EnableTfa { mode: mode.to_string() })
    }

    async fn get(
        &self,
        registry: &ProfileRegistry<'_>,
        properties: &[String],
    ) -> miette::Result<Option<String>> {
        let profile = registry.fetch().await?;
        if self.json {
            return to_pretty_json(&select_json_properties(profile, properties)).map(Some);
        }
        let cleaned = format_profile_map(&profile);
        if properties.is_empty() {
            return render_all_properties(&cleaned, self.parseable).map(Some);
        }
        let properties: Vec<&str> = properties
            .iter()
            .map(String::as_str)
            .collect();
        Ok(Some(render_selected_properties(&cleaned, &properties)))
    }

    async fn set<Sys>(
        &self,
        registry: &ProfileRegistry<'_>,
        property: &str,
        value: Option<String>,
    ) -> miette::Result<Option<String>>
    where
        Sys: PromptPassword + 'static,
    {
        let value = match value {
            Some(value) => Value::String(value),
            None => prompt_password_change::<Sys>().await?,
        };
        // The registry clears the writable fields a request leaves out.
        let mut body = writable_fields(&registry.fetch().await?);
        body.insert(property.to_string(), value);
        let reply = registry.update(&Value::Object(body)).await?;
        // The password is write-only: never print what the registry echoes.
        let result = (property != "password")
            .then(|| reply.get(property))
            .flatten()
            .filter(|value| !value.is_null());
        self.render_set(property, result)
    }

    fn render_set(&self, property: &str, result: Option<&Value>) -> miette::Result<Option<String>> {
        if self.json {
            return to_pretty_json(&json!({ property: result })).map(Some);
        }
        let result = result.map(display_value);
        let text = match (self.parseable, result) {
            (true, result) => format!("{property}\t{}", result.unwrap_or_default()),
            (false, Some(result)) => format!("Set {property} to {result}"),
            (false, None) => format!("Set {property}"),
        };
        Ok(Some(text))
    }

    async fn disable_tfa<Sys>(
        &self,
        registry: &ProfileRegistry<'_>,
    ) -> miette::Result<Option<String>>
    where
        Sys: PromptPassword + 'static,
    {
        if tfa_mode(&registry.fetch().await?).is_none() {
            return Ok(Some("Two factor authentication not enabled.".to_string()));
        }
        let password = read_password::<Sys>("Password: ").await?;
        registry.update(&json!({ "tfa": { "password": password, "mode": "disable" } })).await?;
        if self.json {
            return to_pretty_json(&json!({ "tfa": false })).map(Some);
        }
        if self.parseable {
            return Ok(Some("tfa\tfalse".to_string()));
        }
        Ok(Some("Two factor authentication disabled.".to_string()))
    }
}

fn parse_set(rest: &[String]) -> Result<ProfileCommand, ProfileError> {
    let property = rest
        .first()
        .map(|property| property.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let value = rest
        .get(1..)
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join(" "));
    match (property.as_str(), &value) {
        ("password", Some(_)) => Err(ProfileError::PasswordOnCommandLine),
        ("password", None) => Ok(ProfileCommand::Set { property, value }),
        (_, None) => Err(ProfileError::SetArgsRequired),
        (key, Some(_)) if !WRITABLE_PROFILE_KEYS.contains(&key) => {
            Err(ProfileError::InvalidProperty { property })
        }
        _ => Ok(ProfileCommand::Set { property, value }),
    }
}

/// Keys of `pnpm profile get`, given space- or comma-separated.
fn split_property_list(args: &[String]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim)
        .filter(|property| !property.is_empty())
        .map(str::to_string)
        .collect()
}

fn select_json_properties(profile: Value, properties: &[String]) -> Value {
    if properties.is_empty() {
        return profile;
    }
    let selected = properties
        .iter()
        .map(|property| {
            (
                property.clone(),
                profile
                    .get(property)
                    .cloned()
                    .unwrap_or(Value::Null),
            )
        })
        .collect();
    Value::Object(selected)
}

fn writable_fields(profile: &Value) -> Map<String, Value> {
    WRITABLE_PROFILE_KEYS
        .iter()
        .filter_map(|key| {
            profile
                .get(*key)
                .map(|value| ((*key).to_string(), value.clone()))
        })
        .collect()
}

/// Turn on 2FA, or change its mode. A first-time setup is finished with a
/// code from the authenticator app and prints the recovery codes.
async fn enable_tfa<Sys>(
    registry: &ProfileRegistry<'_>,
    mode: &str,
) -> miette::Result<Option<String>>
where
    Sys: PromptPassword + PromptOtp + 'static,
{
    let profile = registry.fetch().await?;
    if tfa_mode(&profile) == Some(mode) {
        return Ok(Some(format!("Two factor authentication is already enabled and set to {mode}")));
    }
    let password = read_password::<Sys>("Password: ").await?;
    let tfa_pending = profile
        .get("tfa")
        .is_some_and(|tfa| tfa.get("pending").and_then(Value::as_bool) == Some(true));
    if tfa_pending {
        registry.update(&json!({ "tfa": { "password": password, "mode": "disable" } })).await?;
    }
    let challenge =
        registry.update(&json!({ "tfa": { "password": password, "mode": mode } })).await?;
    if challenge
        .get("tfa")
        .and_then(|tfa| tfa.get("mode"))
        .is_some()
    {
        return Ok(Some(format!("Two factor authentication mode changed to: {mode}")));
    }
    confirm_tfa::<Sys>(registry, &challenge).await.map(Some)
}

async fn confirm_tfa<Sys>(
    registry: &ProfileRegistry<'_>,
    challenge: &Value,
) -> miette::Result<String>
where
    Sys: PromptOtp,
{
    let otpauth_url = challenge
        .get("tfa")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("otpauth://"))
        .ok_or_else(|| ProfileError::TfaUnexpectedResponse {
            response: sanitize(
                &challenge
                    .get("tfa")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "undefined".to_string()),
            )
            .into_owned(),
        })?;
    pnpm_diagnostics::println!("{}", authenticator_setup_message(otpauth_url));
    let otp = Sys::input("And an OTP code from your authenticator: ").await
        .into_diagnostic()
        .wrap_err("reading the one-time password")?
        .filter(|otp| !otp.trim().is_empty())
        .ok_or(ProfileError::TfaOtpMissing)?;
    let result = registry.update(&json!({ "tfa": [otp.trim()] })).await?;
    let mut message = "2FA successfully enabled. Below are your recovery codes, please print these out.\nYou will need these to recover access to your account if you lose your authentication device.".to_string();
    for code in result
        .get("tfa")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        message.push_str("\n\t");
        message.push_str(&display_value(code));
    }
    Ok(message)
}

fn authenticator_setup_message(otpauth_url: &str) -> String {
    let secret = url::Url::parse(otpauth_url)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "secret")
                .map(|(_, secret)| secret.into_owned())
        })
        .unwrap_or_default();
    let qr_code = generate_qr_code(otpauth_url).unwrap_or_default();
    sanitize(&format!("Scan into your authenticator app:\n{qr_code}\n Or enter code: {secret}"))
        .into_owned()
}

async fn prompt_password_change<Sys>() -> miette::Result<Value>
where
    Sys: PromptPassword + 'static,
{
    let current = read_password::<Sys>("Current password: ").await?;
    loop {
        let new = read_password::<Sys>("New password: ").await?;
        let confirmed = read_password::<Sys>("       Again:     ").await?;
        if new == confirmed {
            return Ok(json!({ "old": current, "new": new }));
        }
        pnpm_diagnostics::eprintln!("Passwords do not match, please try again.");
    }
}

async fn read_password<Sys>(message: &'static str) -> miette::Result<String>
where
    Sys: PromptPassword + 'static,
{
    tokio::task::spawn_blocking(move || Sys::prompt_password(message)).await
        .into_diagnostic()?
        .into_diagnostic()
        .wrap_err("reading the password")
}

fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn to_pretty_json(value: &Value) -> miette::Result<String> {
    serde_json::to_string_pretty(value).into_diagnostic().wrap_err("serializing profile to JSON")
}
