use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_store_dir::StoreDir;
use std::{
    fs::File,
    io::{BufRead as _, BufReader, Write as _},
    path::Path,
};

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
        let hash = self.hash;

        let Some((_, integrity_hash)) = hash.split_once('-') else {
            return Err(miette::miette!(
                "Invalid hash format. Expected something like sha512-..., got {}",
                hash
            ));
        };

        let decoded = BASE64
            .decode(integrity_hash)
            .into_diagnostic()
            .wrap_err("Failed to decode base64 hash")?;

        use std::fmt::Write as _;
        let mut hex = String::with_capacity(decoded.len() * 2);
        for b in decoded {
            // The hex conversion produces only characters 0-9a-f, which mathematically cannot
            // form path-traversal sequences (like '.', '/', or '\'), ensuring path safety by design.
            write!(&mut hex, "{b:02x}").into_diagnostic()?;
        }

        let config = config()?;
        let store_dir: &StoreDir = &config.store_dir;

        let file_path = store_dir
            .root()
            .join("files")
            .join(&hex[..2])
            .join(&hex[2..]);

        let file = File::open(&file_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("File not found in store: {}", file_path.display()))?;

        copy_to_stdout(file, &file_path)
    }
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
