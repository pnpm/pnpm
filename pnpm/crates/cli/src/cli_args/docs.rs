use clap::Args;
use pnpm_config::Config;
use pnpm_registry::PackageVersion;

/// Open the documentation page of a package in a browser.
#[derive(Debug, Args)]
pub struct DocsArgs {
    /// Package name (optionally with @version).
    pub package: String,
}

impl DocsArgs {
    pub async fn run(self, config: &Config) -> miette::Result<()> {
        let url = self.documentation_url(config).await?;
        open_url(&url)
    }

    async fn documentation_url(&self, config: &Config) -> miette::Result<String> {
        let (_, manifest) =
            super::view::fetch_package_metadata(config, None, &self.package, "docs").await?;
        Ok(documentation_url_from_manifest(&manifest))
    }
}

fn documentation_url_from_manifest(manifest: &PackageVersion) -> String {
    manifest
        .other
        .get("homepage")
        .and_then(serde_json::Value::as_str)
        .filter(|homepage| is_http_url(homepage))
        .map_or_else(|| format!("https://npmx.dev/package/{}", manifest.name), ToString::to_string)
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
            let redacted = url::Url::parse(url).map_or_else(
                |_| url.to_string(),
                |mut parsed_url| {
                    let _ = parsed_url.set_username("");
                    let _ = parsed_url.set_password(None);
                    parsed_url.to_string()
                },
            );
            eprintln!("Could not open browser: {e}");
            println!("{redacted}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
