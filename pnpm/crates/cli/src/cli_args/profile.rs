mod registry;
mod render;
#[cfg(test)]
mod tests;

use crate::cli_args::registry_client::build_registry_client_with_otp_guard;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient, normalize_registry_url};
use registry::{fetch_profile, update_profile};
use render::{format_profile_map, render_all_properties, render_single_property};
use serde_json::{Value, json};

const WRITABLE_PROFILE_KEYS: &[&str] =
    &["email", "password", "fullname", "homepage", "freenode", "twitter", "github"];

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ProfileError {
    #[display("You must be logged in to view or change your profile")]
    #[diagnostic(code(ERR_PNPM_PROFILE_UNAUTHORIZED))]
    Unauthorized,

    #[display(
        "Subcommand is required (get, set, enable-2fa, disable-2fa). Use `pnpm profile get` to view your profile."
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display("Unknown profile command: {command}")]
    #[diagnostic(code(ERR_PNPM_PROFILE_UNKNOWN_COMMAND))]
    UnknownCommand {
        #[error(not(source))]
        command: String,
    },

    #[display(
        r#""{property}" is not a property we can set. Valid properties are: email, password, fullname, homepage, freenode, twitter, github"#
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_INVALID_PROPERTY))]
    InvalidProperty {
        #[error(not(source))]
        property: String,
    },

    #[display("pnpm profile set <prop> <value>")]
    #[diagnostic(code(ERR_PNPM_PROFILE_ARGS_REQUIRED))]
    SetArgsRequired,

    #[display(
        r#"Invalid two-factor authentication mode "{mode}".
Valid modes are:
  auth-only - Require two-factor authentication only when logging in
  auth-and-writes - Require two-factor authentication when logging in AND when publishing"#
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_INVALID_2FA_MODE))]
    Invalid2faMode {
        #[error(not(source))]
        mode: String,
    },

    #[display("You do not have permission to view or modify this profile. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {operation}: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryFailed {
        #[error(not(source))]
        operation: &'static str,
        status: u16,
        #[error(not(source))]
        status_text: String,
        #[error(not(source))]
        body: String,
    },
}

#[derive(Debug, Args)]
pub struct ProfileArgs {
    #[clap(long)]
    pub registry: Option<String>,

    #[clap(long)]
    pub otp: Option<String>,

    #[clap(long)]
    pub json: bool,

    #[clap(short, long)]
    pub parseable: bool,

    pub params: Vec<String>,
}

impl ProfileArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<Option<String>> {
        let registry_url = self.registry.as_deref().unwrap_or(&config.registry);
        let normalized = normalize_registry_url(registry_url);
        let auth_header =
            config.auth_headers.for_url(&normalized).ok_or(ProfileError::Unauthorized)?;
        let http_client = build_registry_client_with_otp_guard(
            config,
            self.otp.as_deref(),
            [normalized.as_ref()],
        )?;
        let mut args = self.params.iter().map(String::as_str);
        let Some(subcommand) = args.next() else {
            return Err(ProfileError::SubcommandRequired.into());
        };
        self.dispatch_subcommand(
            &normalized,
            &http_client,
            &auth_header,
            config.retry_opts(),
            subcommand,
            &mut args,
        )
        .await
    }

    async fn dispatch_subcommand<'a, Iter>(
        &self,
        registry: &str,
        client: &ThrottledClient,
        auth: &str,
        retry: RetryOpts,
        subcommand: &str,
        args: &mut Iter,
    ) -> miette::Result<Option<String>>
    where
        Iter: Iterator<Item = &'a str>,
    {
        match subcommand {
            "get" => self.run_get(registry, client, auth, retry, args.next()).await,
            "set" => self.dispatch_set(registry, client, auth, retry, args).await,
            "enable-2fa" | "enable-tfa" | "enable2fa" | "enabletfa" => {
                let mode = args.next().unwrap_or("auth-and-writes");
                if args.next().is_some() {
                    return Err(ProfileError::Invalid2faMode { mode: mode.to_string() }.into());
                }
                self.run_enable_2fa(registry, client, auth, retry, mode).await
            }
            "disable-2fa" | "disable-tfa" | "disable2fa" | "disabletfa" => {
                self.run_disable_2fa(registry, client, auth, retry).await
            }
            other => Err(ProfileError::UnknownCommand { command: other.to_string() }.into()),
        }
    }

    async fn dispatch_set<'a, Iter>(
        &self,
        registry: &str,
        client: &ThrottledClient,
        auth: &str,
        retry: RetryOpts,
        args: &mut Iter,
    ) -> miette::Result<Option<String>>
    where
        Iter: Iterator<Item = &'a str>,
    {
        let Some(property) = args.next() else {
            return Err(ProfileError::SetArgsRequired.into());
        };
        let value_parts: Vec<&str> = args.collect();
        if value_parts.is_empty() {
            return Err(ProfileError::SetArgsRequired.into());
        }
        let value = value_parts.join(" ");
        self.run_set(registry, client, auth, retry, property, &value).await
    }

    async fn run_get(
        &self,
        registry_url: &str,
        http_client: &ThrottledClient,
        auth_header: &str,
        retry_opts: RetryOpts,
        property: Option<&str>,
    ) -> miette::Result<Option<String>> {
        let profile =
            fetch_profile(registry_url, http_client, auth_header, self.otp.as_deref(), retry_opts)
                .await?;
        if self.json {
            return Self::render_get_json(&profile, property);
        }
        let cleaned = format_profile_map(&profile);
        if let Some(prop) = property {
            return Ok(Some(render_single_property(&cleaned, prop, self.parseable)));
        }
        Ok(Some(render_all_properties(&cleaned, self.parseable)?))
    }

    fn render_get_json(profile: &Value, property: Option<&str>) -> miette::Result<Option<String>> {
        let json_output = if let Some(prop) = property {
            let val = profile
                .get(prop)
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                prop: val,
            })
        } else {
            profile.clone()
        };
        let text = serde_json::to_string_pretty(&json_output)
            .into_diagnostic()
            .wrap_err("serializing profile to JSON")?;
        Ok(Some(text))
    }

    async fn run_set(
        &self,
        registry_url: &str,
        http_client: &ThrottledClient,
        auth_header: &str,
        retry_opts: RetryOpts,
        property: &str,
        value: &str,
    ) -> miette::Result<Option<String>> {
        let prop = property.to_ascii_lowercase();
        if !WRITABLE_PROFILE_KEYS.contains(&prop.as_str()) {
            return Err(ProfileError::InvalidProperty { property: prop }.into());
        }

        let body = json!({
            &prop: value,
        });
        update_profile(
            registry_url,
            http_client,
            auth_header,
            self.otp.as_deref(),
            retry_opts,
            &body,
        )
        .await?;

        if self.json {
            let res = json!({
                &prop: value,
            });
            return Ok(Some(serde_json::to_string_pretty(&res).into_diagnostic()?));
        }

        if self.parseable {
            return Ok(Some(format!("{prop}\t{value}")));
        }

        Ok(Some(format!("Set {prop} to {value}")))
    }

    async fn run_enable_2fa(
        &self,
        registry_url: &str,
        http_client: &ThrottledClient,
        auth_header: &str,
        retry_opts: RetryOpts,
        mode: &str,
    ) -> miette::Result<Option<String>> {
        if mode != "auth-only" && mode != "auth-and-writes" {
            return Err(ProfileError::Invalid2faMode { mode: mode.to_string() }.into());
        }

        let body = json!({
            "tfa": {
                "mode": mode,
            },
        });
        update_profile(
            registry_url,
            http_client,
            auth_header,
            self.otp.as_deref(),
            retry_opts,
            &body,
        )
        .await?;

        if self.json {
            let res = json!({
                "tfa": mode,
            });
            return Ok(Some(serde_json::to_string_pretty(&res).into_diagnostic()?));
        }

        if self.parseable {
            return Ok(Some(format!("tfa\t{mode}")));
        }

        Ok(Some(format!("Two factor authentication mode changed to: {mode}")))
    }

    async fn run_disable_2fa(
        &self,
        registry_url: &str,
        http_client: &ThrottledClient,
        auth_header: &str,
        retry_opts: RetryOpts,
    ) -> miette::Result<Option<String>> {
        let body = json!({
            "tfa": {
                "mode": "disable",
            },
        });
        update_profile(
            registry_url,
            http_client,
            auth_header,
            self.otp.as_deref(),
            retry_opts,
            &body,
        )
        .await?;

        if self.json {
            let res = json!({
                "tfa": false,
            });
            return Ok(Some(serde_json::to_string_pretty(&res).into_diagnostic()?));
        }

        if self.parseable {
            return Ok(Some("tfa\tfalse".to_string()));
        }

        Ok(Some("Two factor authentication disabled.".to_string()))
    }
}
