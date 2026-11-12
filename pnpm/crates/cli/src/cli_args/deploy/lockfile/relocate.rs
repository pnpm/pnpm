use super::{ConvertCtx, Lockfile, PkgName, ReferenceLocation, ResolveBases, convert_package_key};
use crate::cli_args::deploy::resolution::{convert_importer_dep_version, convert_snapshot_dep_ref};
use pnpm_diagnostics::miette::{self, IntoDiagnostic};
use pnpm_lockfile::{
    NamedBindings, NextImporter, NextImporterDep, NextRef, NextSnapshotEntry, NextSnapshots,
    RestoreContext, restore_sections,
};

use std::borrow::Cow;

/// Relocate explicit bindings before rebuilding the peer-suffixed graph.
pub(super) fn prepare_source_lockfile<'a>(
    lockfile: &'a Lockfile,
    ctx: &mut ConvertCtx<'_>,
) -> miette::Result<Cow<'a, Lockfile>> {
    if !lockfile.is_next_format() {
        return Ok(Cow::Borrowed(lockfile));
    }
    let mut sections = lockfile.to_next_sections()?;
    for (id, importer) in &mut sections.importers {
        let link_base = ctx.lockfile_dir.join(id);
        let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: &link_base };
        relocate_importer(importer, ctx, &bases)?;
    }
    sections.snapshots =
        sections.snapshots.map(|snapshots| relocate_snapshots(snapshots, ctx)).transpose()?;
    let mut relocated = lockfile.clone();
    relocated.packages = relocated.packages
        .map(|packages| {
            packages
                .into_iter()
                .map(|(key, metadata)| Ok((convert_package_key(&key, ctx)?, metadata)))
                .collect::<miette::Result<_>>()
        })
        .transpose()?;
    let restored = restore_sections(
        &sections,
        RestoreContext {
            packages: relocated.packages.as_ref(),
            settings: relocated.settings.as_ref(),
            patched_dependencies: relocated.patched_dependencies.as_ref(),
        },
    );
    relocated.importers = restored.importers;
    relocated.snapshots = restored.snapshots;
    ctx.references = ReferenceLocation::Deploy;
    Ok(Cow::Owned(relocated))
}

fn relocate_snapshots(
    snapshots: NextSnapshots,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<NextSnapshots> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: ctx.lockfile_dir };
    let mut relocated = NextSnapshots::new();
    for (key, mut entries) in snapshots {
        for entry in &mut entries {
            relocate_snapshot(entry, ctx, &bases)?;
        }
        relocated
            .entry(convert_package_key(&key, ctx)?)
            .or_default()
            .extend(entries);
    }
    Ok(relocated)
}

fn relocate_importer(
    importer: &mut NextImporter,
    ctx: &ConvertCtx<'_>,
    bases: &ResolveBases<'_>,
) -> miette::Result<()> {
    for (alias, dependency) in [
        &mut importer.dependencies,
        &mut importer.dev_dependencies,
        &mut importer.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    .flatten()
    {
        match dependency {
            NextImporterDep::Resolved(dependency) => {
                dependency.version =
                    convert_importer_dep_version(alias, &dependency.version, ctx, bases)?;
                if let Some(peers) = &mut dependency.peers {
                    relocate_named(peers, ctx)?;
                }
            }
            NextImporterDep::Named(dependency) => relocate_named(&mut dependency.peers, ctx)?,
            NextImporterDep::Specifier(_) => {}
        }
    }
    Ok(())
}

fn relocate_snapshot(
    snapshot: &mut NextSnapshotEntry,
    ctx: &ConvertCtx<'_>,
    bases: &ResolveBases<'_>,
) -> miette::Result<()> {
    for (alias, reference) in [
        &mut snapshot.dependencies,
        &mut snapshot.optional_dependencies,
        &mut snapshot.peer_dependencies,
        &mut snapshot.transitive_peer_dependencies,
    ]
    .into_iter()
    .flatten()
    .flatten()
    {
        relocate_reference(alias, reference, ctx, bases)?;
    }
    Ok(())
}

fn relocate_named(peers: &mut NamedBindings, ctx: &ConvertCtx<'_>) -> miette::Result<()> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: ctx.lockfile_dir };
    for (name, reference) in peers {
        relocate_reference(&name.parse().into_diagnostic()?, reference, ctx, &bases)?;
    }
    Ok(())
}

fn relocate_reference(
    alias: &PkgName,
    reference: &mut NextRef,
    ctx: &ConvertCtx<'_>,
    bases: &ResolveBases<'_>,
) -> miette::Result<()> {
    let mut pending = vec![(alias.clone(), reference)];
    while let Some((alias, reference)) = pending.pop() {
        let version = match reference {
            NextRef::Plain(version) => version,
            NextRef::Named { version, peers } => {
                for (name, reference) in peers {
                    pending.push((name.parse().into_diagnostic()?, reference));
                }
                version
            }
        };
        *version = convert_snapshot_dep_ref(&alias, version, ctx, bases)?;
    }
    Ok(())
}
