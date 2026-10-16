import { isError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { calcPatchHashes, resolvePatchedDependencies } from '@pnpm/lockfile.settings-checker'
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
  /**
   * Reads the lockfiles whose recorded patch hashes a newer patch file is
   * compared with. A missing lockfile is `null`.
   */
  readLockfiles: () => Promise<Array<LockfileObject | null>>
}

export interface PatchesOrHooksStatus {
  issue?: string
  /**
   * A patch file is newer than `lastValidatedTimestamp`, but its content
   * still matches the lockfiles.
   */
  patchesRevalidated: boolean
}

type PatchesStatus = 'unchanged' | 'revalidated' | 'modified'

export async function patchesOrHooksAreModified (opts: PatchesOrHooksAreModifiedOptions): Promise<PatchesOrHooksStatus> {
  const patchesStatus = opts.patchedDependencies ? await getPatchesStatus(opts.patchedDependencies, opts) : 'unchanged'
  if (patchesStatus === 'modified') {
    return { issue: 'Patches were modified', patchesRevalidated: false }
  }
  return {
    issue: findHooksChange(opts),
    patchesRevalidated: patchesStatus === 'revalidated',
  }
}

function findHooksChange (opts: PatchesOrHooksAreModifiedOptions): string | undefined {
  if (opts.ignorePnpmfile) {
    return undefined
  }
  if (!equals(opts.currentPnpmfiles, opts.previousPnpmfiles)) {
    return 'The list of pnpmfiles changed.'
  }
  return findPnpmfileChange(opts.currentPnpmfiles, opts.lastValidatedTimestamp)
}

async function getPatchesStatus (patchedDependencies: Record<string, string>, opts: PatchesOrHooksAreModifiedOptions): Promise<PatchesStatus> {
  const allPatchStats = await Promise.all(Object.values(patchedDependencies).map((patchFile) => {
    return safeStat(patchFile)
  }))
  const anyPatchNewer = allPatchStats.some(
    (patch) =>
      patch && modifiedAtOrAfter(patch, opts.lastValidatedTimestamp)
  )
  if (!anyPatchNewer) return 'unchanged'
  return await patchHashesMatchLockfiles(patchedDependencies, opts.rootDir, opts.readLockfiles) ? 'revalidated' : 'modified'
}

/**
 * A missing lockfile passes here. The checks that follow report it.
 */
async function patchHashesMatchLockfiles (
  patchedDependencies: Record<string, string>,
  rootDir: string,
  readLockfiles: () => Promise<Array<LockfileObject | null>>
): Promise<boolean> {
  let patchHashes: Record<string, string>
  try {
    patchHashes = await calcPatchHashes(resolvePatchedDependencies(patchedDependencies, rootDir) ?? {})
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ERR_PNPM_PATCH_NOT_FOUND') return false
    throw err
  }
  const lockfiles = await readLockfiles()
  return lockfiles.every((lockfile) => lockfile == null || equals(lockfile.patchedDependencies ?? {}, patchHashes))
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
