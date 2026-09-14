use crate::{is_truthy, safe_read_package_json_from_dir};
use serde_json::Value;
use std::path::Path;

/// The file a package ships to have `node-gyp` build it.
pub const BINDING_GYP: &str = "binding.gyp";

/// What a package's manifest and files say about whether it needs a build pass.
///
/// `binding_gyp` is tracked apart from `hooks` because `gypfile: false`
/// silences only the `node-gyp rebuild` install script that a `binding.gyp`
/// implies. See [`manifest_opts_out_of_gyp_build`].
#[derive(Debug, Default, Clone, Copy)]
pub struct BuildTriggers {
    /// The manifest declares a `preinstall`, `install`, or `postinstall`
    /// script that carries a value.
    pub manifest_scripts: bool,
    /// The package ships a [`BINDING_GYP`].
    pub binding_gyp: bool,
    /// The package ships a `.hooks/` entry.
    pub hooks: bool,
    /// The manifest opts out of the implicit `node-gyp rebuild`.
    pub gyp_build_opted_out: bool,
}

impl BuildTriggers {
    #[must_use]
    pub fn requires_build(self) -> bool {
        self.manifest_scripts || self.hooks || (self.binding_gyp && !self.gyp_build_opted_out)
    }

    /// Fold in what one of the package's files says, leaving the
    /// manifest-sourced fields alone.
    pub fn add_file(&mut self, filename: &str) {
        self.add_files(file_path_build_triggers(filename));
    }

    /// Fold in the file-sourced fields of `other`, leaving the
    /// manifest-sourced ones alone.
    pub fn add_files(&mut self, other: BuildTriggers) {
        self.binding_gyp |= other.binding_gyp;
        self.hooks |= other.hooks;
    }

    /// Record what the package's manifest says, leaving the file-sourced
    /// fields alone.
    pub fn read_manifest(&mut self, manifest: &Value) {
        self.manifest_scripts = manifest_requires_build(manifest);
        self.gyp_build_opted_out = manifest_opts_out_of_gyp_build(manifest);
    }
}

/// Whether the manifest opts out of the `node-gyp rebuild` install script
/// pnpm synthesizes for a package that ships a [`BINDING_GYP`] and declares no
/// `install` or `preinstall` script of its own.
///
/// npm documents `gypfile` as that opt-out, and sets `gypfile: true` at publish
/// time on every package it synthesizes the script for, so only the `false`
/// value carries anything pnpm can act on:
/// <https://docs.npmjs.com/cli/v12/configuring-npm/package-json#gypfile>.
///
/// Reading it can only take away a build pnpm would otherwise have run, never
/// add one. `better-sqlite3` v13 sets it and ships a prebuilt binary for every
/// platform it supports, so building it from source needs a Python and C++
/// toolchain that using the package does not.
#[must_use]
pub fn manifest_opts_out_of_gyp_build(manifest: &Value) -> bool {
    manifest.get("gypfile") == Some(&Value::Bool(false))
}

/// The build triggers an extracted package carries on disk: its files, and the
/// manifest that can cancel the [`BINDING_GYP`] one.
///
/// A manifest that cannot be read leaves both of its fields unset — pacquet
/// cannot meaningfully build a package whose extracted content cannot be
/// inspected, and a `binding.gyp` no manifest speaks for is build work, as it is
/// under npm.
#[must_use]
pub fn pkg_build_triggers(pkg_root: &Path) -> BuildTriggers {
    let mut triggers = BuildTriggers {
        manifest_scripts: false,
        binding_gyp: pkg_root.join(BINDING_GYP).exists(),
        hooks: pkg_root.join(".hooks").is_dir(),
        gyp_build_opted_out: false,
    };
    if let Ok(Some(manifest)) = safe_read_package_json_from_dir(pkg_root) {
        triggers.read_manifest(&manifest);
    }
    triggers
}

/// Decide whether a package directory needs a build pass.
///
/// True when the package's manifest declares any of `preinstall`, `install`,
/// or `postinstall`, or when the package contains a `.hooks/` directory, or
/// when it contains a [`BINDING_GYP`] its manifest does not opt out of.
#[must_use]
pub fn pkg_requires_build(pkg_root: &Path) -> bool {
    pkg_build_triggers(pkg_root).requires_build()
}

/// Decide whether a parsed manifest declares lifecycle scripts that
/// make its package a build candidate.
///
/// A script has to carry a value to count, which [`is_truthy`] decides the way
/// pnpm v11's `pkgRequiresBuild` does with `Boolean(manifest.scripts.install)`.
/// An empty `postinstall` runs nothing, so treating the key's presence as build
/// work would ask the user to approve a build that does not exist.
#[must_use]
pub fn manifest_requires_build(manifest: &Value) -> bool {
    manifest
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| {
            ["preinstall", "install", "postinstall"]
                .iter()
                .any(|name| scripts.get(*name).is_some_and(is_truthy))
        })
}

/// Which build triggers a store-index file key implies.
///
/// The two are reported separately so a caller that reads the manifest after
/// the files — every tarball extraction does, since the archive decides the
/// order — can still apply the `gypfile` opt-out.
#[must_use]
fn file_path_build_triggers(filename: &str) -> BuildTriggers {
    BuildTriggers {
        manifest_scripts: false,
        binding_gyp: filename == BINDING_GYP,
        hooks: filename
            .strip_prefix(".hooks")
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\')),
        gyp_build_opted_out: false,
    }
}

#[must_use]
pub fn files_build_triggers<Filenames, Filename>(filenames: Filenames) -> BuildTriggers
where
    Filenames: IntoIterator<Item = Filename>,
    Filename: AsRef<str>,
{
    let mut triggers = BuildTriggers::default();
    for filename in filenames {
        triggers.add_file(filename.as_ref());
    }
    triggers
}
