use crate::HoistedDependencies;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct InstalledHoistedState {
    /// Hoisted-dependencies map produced by the isolated-linker
    /// hoist pass — empty when both hoist patterns are `None` and
    /// always empty under `nodeLinker: hoisted` (the hoisted
    /// linker writes the on-disk tree directly and does not need
    /// the alias-to-`HoistKind` adapter shape).
    pub dependencies: HoistedDependencies,
    /// Per-depPath list of lockfile-relative directory paths the
    /// hoisted linker placed each package at. Empty under the
    /// isolated linker — the field is hoisted-only on disk and
    /// only meaningful when `nodeLinker: hoisted`. Round-trips
    /// through [`pnpm_modules_yaml::Modules::hoisted_locations`]
    /// so a follow-up install (or rebuild) can locate every
    /// package without re-running the walker.
    pub locations: BTreeMap<String, Vec<String>>,
    /// Per-source-project list of virtual-store package directories
    /// its injected `file:` copies were materialized at. Round-trips
    /// through [`pnpm_modules_yaml::Modules::injected_deps`] —
    /// see [`crate::collect_injected_deps`].
    pub injected_deps: BTreeMap<String, Vec<String>>,
}
