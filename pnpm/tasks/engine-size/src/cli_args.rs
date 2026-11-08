use clap::{Parser, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

/// Build the engine-size probe and report its linked size.
#[derive(Debug, Parser)]
#[clap(about)]
pub struct CliArgs {
    /// The link-time optimization modes to build the probe with.
    #[clap(long, value_enum, value_delimiter = ',', default_value = "thin,fat")]
    pub lto: Vec<Lto>,

    /// Registry to install the fixture from. Install timings are skipped
    /// without it.
    #[clap(long)]
    pub registry: Option<String>,

    /// Project to install. It must contain `package.json` and `pnpm-lock.yaml`.
    /// Defaults to the integrated benchmark's fixture.
    #[clap(long)]
    pub fixture: Option<PathBuf>,

    /// Number of installs timed per scenario.
    #[clap(long, default_value_t = 3, value_parser = clap::value_parser!(u16).range(1..))]
    pub runs: u16,

    /// Number of crates and functions listed in the attribution tables.
    #[clap(long, default_value_t = 20)]
    pub top: usize,

    /// Program that strips the symbols from a copy of the probe.
    #[clap(long, default_value = "strip")]
    pub strip_program: PathBuf,

    /// Cargo target directory for the probe builds. Defaults to
    /// `target/engine-size` in the workspace.
    #[clap(long)]
    pub target_dir: Option<PathBuf>,

    /// Also write the report as JSON to this path.
    #[clap(long)]
    pub json: Option<PathBuf>,
}

impl CliArgs {
    /// The target directory as an absolute path, so cargo, which runs in
    /// the workspace root, and this tool resolve it to the same place.
    pub fn target_dir(&self) -> Result<PathBuf, String> {
        let target_dir = self.target_dir
            .clone()
            .unwrap_or_else(|| workspace_root().join("target").join("engine-size"));
        std::path::absolute(&target_dir)
            .map_err(|error| format!("resolve {}: {error}", target_dir.display()))
    }

    pub fn fixture(&self) -> PathBuf {
        self.fixture
            .clone()
            .unwrap_or_else(|| {
                workspace_root().join("pnpm/tasks/integrated-benchmark/src/fixtures")
            })
    }
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Lto {
    Thin,
    Fat,
}

impl Lto {
    pub const fn as_str(self) -> &'static str {
        match self {
            Lto::Thin => "thin",
            Lto::Fat => "fat",
        }
    }
}

#[cfg(test)]
mod tests;
