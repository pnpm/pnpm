import { WANTED_LOCKFILE } from '@pnpm/constants'
import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  getPeerSatisfactionEdgesToSkip,
  omitUnretainedPeerSatisfactionEdges,
} from '@pnpm/lockfile.peer-edges'
import type {
  LockfileObject,
  PackageSnapshots,
} from '@pnpm/lockfile.types'
import { lockfileWalker, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { logger } from '@pnpm/logger'
import type { DependenciesField, DepPath, ProjectId } from '@pnpm/types'

import { filterImporter } from './filterImporter.js'

const lockfileLogger = logger('lockfile')

export function filterLockfileByImporters (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<DepPath>
    skipRuntimes?: boolean
    failOnMissingDependencies: boolean
    resolvePeersFromWorkspaceRoot?: boolean
  }
): LockfileObject {
  const importers = { ...lockfile.importers }
  for (const importerId of importerIds) {
    importers[importerId] = filterImporter(lockfile.importers[importerId], opts.include, { skipRuntimes: opts.skipRuntimes })
  }

  // Classified on the unfiltered importers: the filtered copy lacks the
  // excluded dependency fields that decide whether an importer lists a peer.
  const peerSatisfactionEdges = getPeerSatisfactionEdgesToSkip(lockfile, opts)
  let packages = {} as PackageSnapshots
  if (lockfile.packages != null) {
    pkgAllDeps(
      lockfileWalker(
        { ...lockfile, importers },
        importerIds,
        { include: opts.include, skipped: opts.skipped, peerSatisfactionEdges: peerSatisfactionEdges ?? new Map() }
      ).step,
      packages,
      {
        failOnMissingDependencies: opts.failOnMissingDependencies,
      }
    )
    packages = omitUnretainedPeerSatisfactionEdges(packages, peerSatisfactionEdges)
  }

  return {
    ...lockfile,
    importers,
    packages,
  }
}

function pkgAllDeps (
  step: LockfileWalkerStep,
  pickedPackages: PackageSnapshots,
  opts: {
    failOnMissingDependencies: boolean
  }
) {
  for (const { pkgSnapshot, depPath, next } of step.dependencies) {
    pickedPackages[depPath] = pkgSnapshot
    pkgAllDeps(next(), pickedPackages, opts)
  }
  for (const depPath of step.missing) {
    if (opts.failOnMissingDependencies) {
      throw new LockfileMissingDependencyError(depPath)
    }
    lockfileLogger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
  }
}
