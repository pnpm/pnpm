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
    #[clap(long, default_value_t = 3)]
    pub runs: usize,

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
    pub fn target_dir(&self) -> PathBuf {
        self.target_dir
            .clone()
            .unwrap_or_else(|| workspace_root().join("target").join("engine-size"))
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
