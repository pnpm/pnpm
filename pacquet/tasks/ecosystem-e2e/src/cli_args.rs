use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Install real-world JavaScript stacks with pnpm and pacquet across
/// `node_modules` layouts, then build each app to prove the produced layout
/// actually works. The cross product of `--binary` × `--layout` × stacks is
/// the grid: the binary axis catches pnpm↔pacquet parity gaps, the layout
/// axis catches breakage introduced by the global virtual store.
#[derive(Debug, Parser)]
pub struct CliArgs {
    /// Path to the pnpm executable. Always required: pnpm scaffolds every
    /// stack (via `pnpm dlx`) regardless of which binary installs it.
    #[clap(long, default_value = "pnpm")]
    pub pnpm: String,

    /// Path to the pacquet executable. Only required when `--binary`
    /// includes pacquet.
    #[clap(long, default_value = "pacquet")]
    pub pacquet: String,

    /// Which package managers perform the install.
    #[clap(long, value_enum, default_value_t = BinaryChoice::Both)]
    pub binary: BinaryChoice,

    /// Which `node_modules` layouts to exercise.
    #[clap(long, value_enum, default_value_t = LayoutChoice::Both)]
    pub layout: LayoutChoice,

    /// Restrict the run to stacks whose name matches (repeatable). Defaults
    /// to every known stack.
    #[clap(long = "stack")]
    pub stacks: Vec<String>,

    /// Directory holding the scaffolded templates and per-cell work trees.
    /// Wiped at the start of every run unless `--keep` is passed.
    #[clap(long, default_value = "ecosystem-e2e-work")]
    pub work_dir: PathBuf,

    /// Keep the work directory from a previous run instead of wiping it, so
    /// already-scaffolded templates can be reused while iterating locally.
    #[clap(long)]
    pub keep: bool,

    /// Stop after the build stage instead of booting and probing each app.
    /// The serve stage is what proves the layout works at runtime, so this
    /// is for quick iteration only.
    #[clap(long)]
    pub skip_serve: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BinaryChoice {
    Pnpm,
    Pacquet,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LayoutChoice {
    Isolated,
    GlobalVirtualStore,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Pnpm,
    Pacquet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Isolated,
    GlobalVirtualStore,
}

impl Binary {
    pub fn label(self) -> &'static str {
        match self {
            Binary::Pnpm => "pnpm",
            Binary::Pacquet => "pacquet",
        }
    }
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Isolated => "isolated",
            Layout::GlobalVirtualStore => "global-virtual-store",
        }
    }

    /// Value written for `enableGlobalVirtualStore` in the cell's
    /// `pnpm-workspace.yaml`.
    pub fn enable_global_virtual_store(self) -> bool {
        matches!(self, Layout::GlobalVirtualStore)
    }
}

impl BinaryChoice {
    pub fn expand(self) -> Vec<Binary> {
        match self {
            BinaryChoice::Pnpm => vec![Binary::Pnpm],
            BinaryChoice::Pacquet => vec![Binary::Pacquet],
            BinaryChoice::Both => vec![Binary::Pnpm, Binary::Pacquet],
        }
    }
}

impl LayoutChoice {
    pub fn expand(self) -> Vec<Layout> {
        match self {
            LayoutChoice::Isolated => vec![Layout::Isolated],
            LayoutChoice::GlobalVirtualStore => vec![Layout::GlobalVirtualStore],
            LayoutChoice::Both => vec![Layout::Isolated, Layout::GlobalVirtualStore],
        }
    }
}
