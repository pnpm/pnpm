use crate::cli_args::sanitize::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use owo_colors::{OwoColorize, Rgb, Stream};
use pacquet_config::Config;
use pacquet_store_dir::{
    decode_package_files_index,
    store_index::{StoreIndex, StoreIndexError},
    transcode_to_plain_msgpack,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindHashError {
    #[display("No package or index file matching this hash was found.")]
    #[diagnostic(code(ERR_PNPM_INVALID_FILE_HASH))]
    InvalidFileHash,

    #[display("{source}")]
    #[diagnostic(transparent)]
    StoreIndex {
        #[error(source)]
        source: StoreIndexError,
    },

    #[display("Failed to decode package_index row {key:?}: {source}")]
    CorruptStoreIndexRow {
        key: String,
        #[error(source)]
        source: StoreIndexError,
    },
}

#[derive(Debug, Args)]
pub struct FindHashArgs {
    /// The hash of the file to search for. Can be a hex string or shaN-base64 format.
    pub hash: String,
}

impl From<StoreIndexError> for FindHashError {
    fn from(source: StoreIndexError) -> Self {
        Self::StoreIndex { source }
    }
}

const EXPECTED_HEX_LENGTH: usize = 128;
const EXPECTED_SHA512_BYTES: usize = 64;
const MAX_SHA512_BASE64_LENGTH: usize = 88;

impl FindHashArgs {
    pub fn run<'a>(
        self,
        config: impl FnOnce() -> miette::Result<&'a Config>,
    ) -> miette::Result<()> {
        let hash = parse_hash(self.hash)?;

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

        let mut results = Vec::new();

        store_index.for_each_raw(|index_key, bytes| -> Result<(), FindHashError> {
            let data = decode_find_hash_index(&bytes).map_err(|source| {
                FindHashError::CorruptStoreIndexRow { key: index_key.clone(), source }
            })?;
            if !contains_hash(&data, &hash) {
                return Ok(());
            }

            let (name, version) = package_identity(&bytes).map_err(|source| {
                FindHashError::CorruptStoreIndexRow { key: index_key.clone(), source }
            })?;
            results.push((name, version, index_key));
            Ok(())
        })?;

        if results.is_empty() {
            return Err(FindHashError::InvalidFileHash.into());
        }

        for (name, version, index_key) in results {
            println!(
                "{}@{}  {}",
                package_info(&name),
                package_info(&version),
                index_path(&index_key),
            );
        }

        Ok(())
    }
}

fn parse_hash(mut hash: String) -> miette::Result<String> {
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
        if base64_part.len() > MAX_SHA512_BASE64_LENGTH {
            return Err(miette::miette!(
                "Invalid hash format: sha512 base64 payload has {} character(s), expected at most {MAX_SHA512_BASE64_LENGTH}.",
                base64_part.len(),
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
        if decoded.len() != EXPECTED_SHA512_BYTES {
            return Err(miette::miette!(
                "Decoded hash is {} bytes, expected {EXPECTED_SHA512_BYTES} bytes for sha512.",
                decoded.len(),
            ));
        }
        use std::fmt::Write as _;
        let mut hex = String::with_capacity(decoded.len() * 2);
        for b in decoded {
            write!(&mut hex, "{b:02x}").into_diagnostic()?;
        }
        return Ok(hex);
    }

    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(miette::miette!(
            "Invalid hash format: \"{hash}\" contains non-hexadecimal characters. \
             Expected a 128-character hex string or a sha512-base64 format."
        ));
    }
    if hash.len() != EXPECTED_HEX_LENGTH {
        return Err(miette::miette!(
            "Invalid hash format: \"{hash}\" has {} character(s), expected {EXPECTED_HEX_LENGTH}.",
            hash.len(),
        ));
    }
    hash.make_ascii_lowercase();
    Ok(hash)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindHashPackageIndex {
    algo: String,
    files: HashMap<String, FindHashFileInfo>,
    side_effects: Option<HashMap<String, FindHashSideEffectsDiff>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindHashFileInfo {
    digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindHashSideEffectsDiff {
    added: Option<HashMap<String, FindHashFileInfo>>,
}

fn decode_find_hash_index(bytes: &[u8]) -> Result<FindHashPackageIndex, StoreIndexError> {
    let plain = transcode_to_plain_msgpack(bytes)
        .map_err(|source| StoreIndexError::Transcode { source })?;
    rmp_serde::from_slice(&plain).map_err(|source| StoreIndexError::Decode { source })
}

fn contains_hash(data: &FindHashPackageIndex, hash: &str) -> bool {
    data.algo == "sha512"
        && (data.files.values().any(|file| file.digest == hash)
            || data.side_effects.as_ref().is_some_and(|side_effects| {
                side_effects.values().any(|side_effect| {
                    side_effect
                        .added
                        .as_ref()
                        .is_some_and(|added| added.values().any(|file| file.digest == hash))
                })
            }))
}

fn package_identity(bytes: &[u8]) -> Result<(String, String), StoreIndexError> {
    let data = decode_package_files_index(bytes)?;
    let name = data
        .manifest
        .as_ref()
        .and_then(|manifest| {
            manifest.get("name").and_then(|n| n.as_str()).map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let version = data
        .manifest
        .as_ref()
        .and_then(|manifest| {
            manifest.get("version").and_then(|n| n.as_str()).map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    Ok((name, version))
}

/// Color a package name/version like pnpm's `PACKAGE_INFO_CLR = chalk.greenBright`.
/// `chalk` suppresses color when stdout is not a TTY, so this only emits ANSI
/// when stdout supports color.
fn package_info(text: &str) -> String {
    sanitize(text).as_ref().if_supports_color(Stream::Stdout, |t| t.bright_green()).to_string()
}

/// Color an index key like pnpm's `INDEX_PATH_CLR = chalk.hex('#078487')`
/// (`#078487` is `rgb(7, 132, 135)`). See [`package_info`] for the TTY behavior.
fn index_path(text: &str) -> String {
    sanitize(text)
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.color(Rgb(7, 132, 135)))
        .to_string()
}
