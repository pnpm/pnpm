use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, STANDARD_NO_PAD as BASE64_NO_PAD},
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use std::{
    fmt::Write as _,
    fs::File,
    io::{self, BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
enum CatFileError {
    #[display("Invalid hash format. Expected something like sha512-..., got {hash}")]
    #[diagnostic(code(ERR_PNPM_INVALID_HASH))]
    InvalidHashFormat {
        #[error(not(source))]
        hash: String,
    },

    #[display("Failed to decode base64 hash")]
    #[diagnostic(code(ERR_PNPM_INVALID_HASH))]
    DecodeBase64 {
        #[error(source)]
        source: base64::DecodeError,
    },

    #[display("File not found in store: {}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_HASH))]
    NotFound {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

#[derive(Debug, Args)]
pub struct CatFileArgs {
    /// The hash of the file to read (e.g. sha512-...)
    pub hash: String,
}

impl CatFileArgs {
    pub fn run<'a>(
        self,
        config: impl FnOnce() -> miette::Result<&'a Config>,
    ) -> miette::Result<()> {
        let hex = decode_hex_digest(&self.hash)?;
        let config = config()?;
        let file_path = config.store_dir
            .cas_file_path_by_mode(&hex, 0)
            .ok_or(CatFileError::InvalidHashFormat { hash: self.hash })?;

        let file = match File::open(&file_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CatFileError::NotFound { path: file_path, source: error }.into());
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to read {}", file_path.display()));
            }
        };

        copy_to_stdout(file, &file_path)
    }
}

fn decode_hex_digest(hash: &str) -> miette::Result<String> {
    let Some((algorithm, integrity_hash)) = hash.split_once('-') else {
        return Err(CatFileError::InvalidHashFormat { hash: hash.to_string() }.into());
    };
    if algorithm.is_empty() || integrity_hash.is_empty() {
        return Err(CatFileError::InvalidHashFormat { hash: hash.to_string() }.into());
    }
    let decoded = BASE64
        .decode(integrity_hash)
        .or_else(|_| BASE64_NO_PAD.decode(integrity_hash))
        .map_err(|source| CatFileError::DecodeBase64 { source })?;
    let mut hex = String::with_capacity(decoded.len() * 2);
    for byte in decoded {
        write!(&mut hex, "{byte:02x}").into_diagnostic()?;
    }
    if hex.len() <= 2 {
        return Err(CatFileError::InvalidHashFormat { hash: hash.to_string() }.into());
    }
    Ok(hex)
}

/// Copy `file` to stdout. A failed read fails with `file_path`. A failed
/// write goes to [`pnpm_diagnostics::stdout::check`].
fn copy_to_stdout(file: File, file_path: &Path) -> miette::Result<()> {
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut stdout = std::io::stdout().lock();
    loop {
        let chunk = reader
            .fill_buf()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to read {}", file_path.display()))?;
        if chunk.is_empty() {
            return Ok(());
        }
        let length = chunk.len();
        if let Err(error) = stdout.write_all(chunk) {
            pnpm_diagnostics::stdout::check(Err::<(), _>(error));
            return Ok(());
        }
        reader.consume(length);
    }
}
