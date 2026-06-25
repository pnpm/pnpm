use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_package_manifest::PackageManifest;
use pacquet_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
    pick_registry_for_package,
};
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;

/// Open the documentation of a package (or `pnpm home <pkg>`).
#[derive(Debug, Args)]
pub struct DocsArgs {
    /// Package name (optionally with @version).
    pub package: String,
}

impl DocsArgs {
    pub async fn run(self, config: &Config) -> miette::Result<()> {
        let raw_spec = &self.package;

        let parsed = parse_wanted_dependency(raw_spec);
        let name = parsed.alias.as_deref().unwrap_or(raw_spec);
        let (resolved_name, _range) = PackageManifest::resolve_registry_dependency(name, name);

        let http_client = ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .into_diagnostic()
        .wrap_err("create the network client for docs")?;

        let registries: std::collections::HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        let registry = pick_registry_for_package(&registries, resolved_name, None);

        let outcome = fetch_full_metadata(
            resolved_name,
            &FetchFullMetadataOptions {
                registry: &registry,
                http_client: &http_client,
                auth_headers: &config.auth_headers,
                full_metadata: true,
                etag: None,
                modified: None,
                retry_opts: RetryOpts::default(),
            },
        )
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("fetch package info for {raw_spec}"))?;

        let package = match outcome {
            FetchFullMetadataOutcome::Modified(pkg) => *pkg,
            FetchFullMetadataOutcome::NotModified => {
                unreachable!("unexpected 304 without conditional headers")
            }
        };

        let fallback = || format!("https://npmx.dev/package/{}", package.name);
        let url = package
            .homepage
            .as_deref()
            .filter(|s| is_http_url(s))
            .map_or_else(fallback, ToString::to_string);

        open_url(&url)
    }
}

fn is_http_url(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    url::Url::parse(value)
        .is_ok_and(|parsed| parsed.scheme() == "http" || parsed.scheme() == "https")
}

fn open_url(url: &str) -> miette::Result<()> {
    let result = {
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open").arg(url).spawn()
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(url).spawn()
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY: ShellExecuteW invokes the default handler for the
            // URL protocol without going through cmd, avoiding the shell
            // metacharacter injection that `cmd /c start` is vulnerable to.
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;

            let url_wide: Vec<u16> =
                OsStr::new(url).encode_wide().chain(std::iter::once(0)).collect();

            let result = unsafe {
                windows_sys::Win32::UI::Shell::ShellExecuteW(
                    std::ptr::null_mut(), // hwnd
                    std::ptr::null(),     // lpOperation (null => "open")
                    url_wide.as_ptr(),    // lpFile
                    std::ptr::null(),     // lpParameters
                    std::ptr::null(),     // lpDirectory
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                )
            };
            if (result as isize) > 32 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // On unsupported platforms, just print the URL.
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported platform"))
        }
    };
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            // If we can't open the browser, print the URL instead.
            eprintln!("Could not open browser: {e}");
            println!("{url}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
