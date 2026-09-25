use pnpm_config::Config;
use pnpm_publish::{Access, OidcHttpOptions, PublishPackedPkgOptions};

use super::PublishArgs;

impl PublishArgs {
    /// Runs before git checks, packing, and any upload, so a rejected flag
    /// combination changes nothing.
    pub(super) fn validate_publish_flags(
        &self,
        config: &Config,
        recursive: bool,
        stage: bool,
    ) -> miette::Result<()> {
        if stage {
            self.publish_options(config, None, stage).validate()?;
        }
        if self.flags.batch && !recursive {
            return Err(miette::miette!(
                code = "ERR_PNPM_BATCH_PUBLISH_REQUIRES_RECURSIVE",
                help = r#"Run "pnpm publish -r --batch" to publish all workspace packages in a single request."#,
                "--batch can only be used together with --recursive",
            ));
        }
        self.validate_new_version()?;
        Ok(())
    }

    /// Staging ignores a configured `publishWaitTimeout`; only an explicit
    /// flag reaches [`PublishPackedPkgOptions::validate`] there.
    pub(super) fn publish_options(
        &self,
        config: &Config,
        otp: Option<String>,
        stage: bool,
    ) -> PublishPackedPkgOptions {
        let default_wait_timeout = if stage { 0 } else { config.publish_wait_timeout };
        PublishPackedPkgOptions {
            dry_run: self.flags.dry_run,
            stage,
            wait_timeout: std::time::Duration::from_millis(
                self.flags.registry.publish_wait_timeout.unwrap_or(default_wait_timeout),
            ),
            registry: pnpm_publish::PublishRegistryOptions {
                default: config.registry.clone(),
                scoped: config.registries_by_scope.clone(),
                access: self.flags.registry.access.as_deref().and_then(Access::parse),
                tag: self.flags.registry.tag.clone().unwrap_or_else(|| "latest".to_owned()),
                otp,
                // An absent `--provenance` leaves the decision to the OIDC flow.
                provenance: self.flags.registry.provenance.then_some(true),
                http: OidcHttpOptions {
                    fetch_retries: Some(config.fetch_retries),
                    fetch_retry_factor: Some(f64::from(config.fetch_retry_factor)),
                    fetch_retry_maxtimeout: Some(config.fetch_retry_maxtimeout),
                    fetch_retry_mintimeout: Some(config.fetch_retry_mintimeout),
                    fetch_timeout: Some(config.fetch_timeout),
                },
            },
        }
    }
}
