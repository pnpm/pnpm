import { equals } from 'ramda'

import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { safeStat, safeStatSync } from './safeStat.js'

export interface PatchesOrHooksAreModifiedOptions {
  patchedDependencies?: Record<string, string>
  rootDir: string
  lastValidatedTimestamp: number
  currentPnpmfiles: string[]
  previousPnpmfiles: string[]
  ignorePnpmfile?: boolean
}

export async function patchesOrHooksAreModified (opts: PatchesOrHooksAreModifiedOptions): Promise<string | undefined> {
  if (opts.patchedDependencies && await patchesAreModified(opts.patchedDependencies, opts.lastValidatedTimestamp)) {
    return 'Patches were modified'
  }
  if (opts.ignorePnpmfile) {
    return undefined
  }
  if (!equals(opts.currentPnpmfiles, opts.previousPnpmfiles)) {
    return 'The list of pnpmfiles changed.'
  }
  return findPnpmfileChange(opts.currentPnpmfiles, opts.lastValidatedTimestamp)
}

async function patchesAreModified (patchedDependencies: Record<string, string>, lastValidatedTimestamp: number): Promise<boolean> {
  const allPatchStats = await Promise.all(Object.values(patchedDependencies).map((patchFile) => {
    return safeStat(patchFile)
  }))
  return allPatchStats.some(
    (patch) =>
      patch && modifiedAtOrAfter(patch, lastValidatedTimestamp)
  )
}

function findPnpmfileChange (pnpmfilePaths: string[], lastValidatedTimestamp: number): string | undefined {
  for (const pnpmfilePath of pnpmfilePaths) {
    const pnpmfileStats = safeStatSync(pnpmfilePath)
    if (pnpmfileStats == null) {
      return `pnpmfile at "${pnpmfilePath}" was removed`
    }
    if (modifiedAtOrAfter(pnpmfileStats, lastValidatedTimestamp)) {
      return `pnpmfile at "${pnpmfilePath}" was modified`
    }
  }
  return undefined
}
