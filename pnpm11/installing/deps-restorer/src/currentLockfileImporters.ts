import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS, type DepPath, type ProjectId } from '@pnpm/types'

/**
 * The importers a previous install materialized: every package the current
 * lockfile records for them is in it. The current lockfile lists every
 * importer of the wanted lockfile, including those a selected install left
 * out, and a left-out importer can share some packages with a selected one.
 */
export function pickMaterializedImporterIds<Id extends string> (currentLockfile: LockfileObject, importerIds: Id[]): Id[] {
  return importerIds.filter((importerId) => {
    const depPaths = recordedDepPaths(currentLockfile, importerId)
    return depPaths.length > 0 && allInstalled(depPaths, currentLockfile.packages)
  })
}

/**
 * The importers whose recorded dependencies the current lockfile holds a
 * package for, the only ones the previous hoisted layout can be rebuilt from.
 */
export function pickResolvableImporterIds<Id extends string> (currentLockfile: LockfileObject, importerIds: Id[]): Id[] {
  return importerIds.filter((importerId) =>
    allInstalled(recordedDepPaths(currentLockfile, importerId), currentLockfile.packages)
  )
}

function allInstalled (depPaths: DepPath[], packages: PackageSnapshots | undefined): boolean {
  return depPaths.every((depPath) => packages?.[depPath] != null)
}

function recordedDepPaths (currentLockfile: LockfileObject, importerId: string): DepPath[] {
  const snapshot: ProjectSnapshot | undefined = currentLockfile.importers[importerId as ProjectId]
  if (snapshot == null) return []
  return DEPENDENCIES_FIELDS.flatMap((depType) =>
    Object.entries(snapshot[depType] ?? {})
      .map(([alias, ref]) => dp.refToRelative(ref, alias))
      .filter((depPath): depPath is DepPath => depPath != null)
  )
}
