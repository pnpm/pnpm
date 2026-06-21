use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use owo_colors::{OwoColorize, Rgb};
use pacquet_config::Config;
use pacquet_store_dir::store_index::StoreIndex;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindHashError {
    #[display("No package or index file matching this hash was found.")]
    #[diagnostic(code(ERR_PNPM_INVALID_FILE_HASH))]
    InvalidFileHash,
}

#[derive(Debug, Args)]
pub struct FindHashArgs {
    /// The hash of the file to search for. Can be a hex string or shaN-base64 format.
    pub hash: String,
}

const EXPECTED_HEX_LENGTH: usize = 128;

impl FindHashArgs {
    pub fn run<'a>(
        self,
        config: impl FnOnce() -> miette::Result<&'a Config>,
    ) -> miette::Result<()> {
        let mut hash = self.hash;

        if hash.contains('-') {
            let Some((algo, base64_part)) = hash.split_once('-') else {
                return Err(miette::miette!(
                    "Invalid hash format. Expected something like sha512-..., got {}",
                    hash
                ));
            };
            if !algo.eq_ignore_ascii_case("sha512") {
                return Err(miette::miette!(
                    "Unsupported hash algorithm \"{algo}\". Only \"sha512\" is supported."
                ));
            }
            use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
            let decoded = BASE64
                .decode(base64_part)
                .or_else(|_| {
                    use base64::{
                        Engine as _, engine::general_purpose::STANDARD_NO_PAD as BASE64_NO_PAD,
                    };
                    BASE64_NO_PAD.decode(base64_part)
                })
                .into_diagnostic()
                .wrap_err("Failed to decode base64 hash")?;
            if decoded.len() != 64 {
                return Err(miette::miette!(
                    "Decoded hash is {} bytes, expected 64 bytes for sha512.",
                    decoded.len(),
                ));
            }
            use std::fmt::Write as _;
            let mut hex = String::with_capacity(decoded.len() * 2);
            for b in decoded {
                write!(&mut hex, "{b:02x}").into_diagnostic()?;
            }
            hash = hex;
        } else if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(miette::miette!(
                "Invalid hash format: \"{hash}\" contains non-hexadecimal characters. \
                 Expected a 128-character hex string or a sha512-base64 format."
            ));
        } else if hash.len() != EXPECTED_HEX_LENGTH {
            return Err(miette::miette!(
                "Invalid hash format: \"{hash}\" has {} character(s), expected {EXPECTED_HEX_LENGTH}.",
                hash.len(),
            ));
        }
        hash = hash.to_lowercase();

        let config = config()?;
        let store_dir = &config.store_dir;

        let store_index = if config.frozen_store {
            StoreIndex::open_immutable(store_dir.root())
                .into_diagnostic()
                .wrap_err("Failed to open store index (frozen)")?
        } else {
            StoreIndex::open_readonly_in(store_dir)
                .into_diagnostic()
                .wrap_err("Failed to open store index")?
        };

        let keys =
            store_index.keys().into_diagnostic().wrap_err("Failed to get store index keys")?;
        let mut results = Vec::new();

        for chunk in keys.chunks(999) {
            let entries = store_index.get_many(chunk).into_diagnostic()?;
            for (index_key, data) in entries {
                let mut found = false;

                for file in data.files.values() {
                    if file.digest == hash {
                        found = true;
                        break;
                    }
                }

                if !found && let Some(side_effects) = &data.side_effects {
                    for side_effect in side_effects.values() {
                        if let Some(added) = &side_effect.added {
                            for file in added.values() {
                                if file.digest == hash {
                                    found = true;
                                    break;
                                }
                            }
                        }
                        if found {
                            break;
                        }
                    }
                }

                if found {
                    let name = data
                        .manifest
                        .as_ref()
                        .and_then(|manifest| {
                            manifest
                                .get("name")
                                .and_then(|n| n.as_str())
                                .map(std::string::ToString::to_string)
                        })
                        .unwrap_or_else(|| "unknown".to_string());
                    let version = data
                        .manifest
                        .as_ref()
                        .and_then(|manifest| {
                            manifest
                                .get("version")
                                .and_then(|n| n.as_str())
                                .map(std::string::ToString::to_string)
                        })
                        .unwrap_or_else(|| "unknown".to_string());
                    results.push((name, version, index_key.clone()));
                }
            }
        }

        if results.is_empty() {
            return Err(FindHashError::InvalidFileHash.into());
        }

        // pnpm uses PACKAGE_INFO_CLR = chalk.greenBright and INDEX_PATH_CLR = chalk.hex(`#078487`)
        // We will use OwoColorize to match. `#078487` is rgb(7, 132, 135)
        for (name, version, index_key) in results {
            println!(
                "{}@{}  {}",
                name.bright_green(),
                version.bright_green(),
                index_key.color(Rgb(7, 132, 135)),
            );
        }

        Ok(())
    }
}
