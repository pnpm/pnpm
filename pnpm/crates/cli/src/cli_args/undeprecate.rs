use clap::Args;
use pacquet_config::Config;

use super::deprecate::{
    DeprecateContext, DeprecateError, PackageSpec, parse_package_spec, update_deprecation,
};

#[derive(Debug, Args)]
pub struct UndeprecateArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// The package name.
    pub params: Vec<String>,
}

impl UndeprecateArgs {
    pub async fn run(self, config: &Config) -> miette::Result<Option<String>> {
        let context = DeprecateContext::new(config, self.registry.as_ref(), self.otp)?;

        let spec = self.params.first().ok_or(DeprecateError::PackageRequired)?;
        let PackageSpec { name: package_name, version } = parse_package_spec(spec)?;

        let output = update_deprecation(&context, None, &package_name, version.as_deref()).await?;
        Ok(Some(output))
    }
}
