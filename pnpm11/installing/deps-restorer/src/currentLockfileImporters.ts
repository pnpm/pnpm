import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS, type DepPath, type ProjectId } from '@pnpm/types'

/**
 * The importers a previous install materialized a package for. The current
 * lockfile lists every importer of the wanted lockfile, including those a
 * selected install left out, so the importer entry alone does not say that
 * anything of it is on disk.
 */
export function pickMaterializedImporterIds<Id extends string> (currentLockfile: LockfileObject, importerIds: Id[]): Id[] {
  const packages = currentLockfile.packages ?? {}
  return importerIds.filter((importerId) =>
    recordedDepPaths(currentLockfile.importers[importerId as string as ProjectId]).some((depPath) => packages[depPath] != null)
  )
}

/**
 * The importers whose recorded dependencies the current lockfile holds a
 * package for, the only ones the previous hoisted layout can be rebuilt from.
 */
export function pickResolvableImporterIds<Id extends string> (currentLockfile: LockfileObject, importerIds: Id[]): Id[] {
  const packages = currentLockfile.packages ?? {}
  return importerIds.filter((importerId) =>
    recordedDepPaths(currentLockfile.importers[importerId as string as ProjectId]).every((depPath) => packages[depPath] != null)
  )
}

function recordedDepPaths (snapshot: ProjectSnapshot | undefined): DepPath[] {
  if (snapshot == null) return []
  return DEPENDENCIES_FIELDS.flatMap((depType) =>
    Object.entries(snapshot[depType] ?? {})
      .map(([alias, ref]) => dp.refToRelative(ref, alias))
      .filter((depPath): depPath is DepPath => depPath != null)
  )
}
