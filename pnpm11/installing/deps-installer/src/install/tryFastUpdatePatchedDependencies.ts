import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import {
  allPatchKeys,
  getPatchInfo,
  groupPatchedDependencies,
  type PatchGroupRecord,
} from '@pnpm/patching.config'

export interface FastPatchedDependenciesUpdateOptions {
  patchedDependencies: Record<string, string> | undefined
  allowUnusedPatches: boolean
}

/**
 * Record a changed `patchedDependencies` without resolving the dependency
 * graph, for the changes that touch no package the lockfile records.
 *
 * A patch that matches a locked package contributes a `(patch_hash=...)`
 * segment to that package's key, so adding, removing, or editing such a patch
 * rekeys the graph and has to go through the resolver. A patch key that
 * matches nothing contributes nothing, which makes the recorded map the only
 * thing that changes.
 *
 * Returns `false` — nothing changed, a changed key matches a locked package,
 * or the new configuration leaves a patch unused while `allowUnusedPatches` is
 * off — which keeps the full-resolution path, where `ERR_PNPM_UNUSED_PATCH` is
 * raised.
 */
export function tryFastUpdatePatchedDependencies (
  lockfile: LockfileObject,
  opts: FastPatchedDependenciesUpdateOptions
): boolean {
  const recorded = lockfile.patchedDependencies ?? {}
  const current = opts.patchedDependencies ?? {}
  // Every key whose presence or hash differs, taken from the recorded map as
  // well as the current one. A key the previous install applied still owns a
  // `(patch_hash=...)` segment in the recorded graph, so dropping it rekeys
  // that package just as adding it did.
  const affected = changedKeys(recorded, current)
  if (affected.length === 0) return false

  const affectedGroups = groupsFromKeys(affected)
  if (affectedGroups == null) return false
  const affectedApplied = appliedPatchKeys(lockfile, affectedGroups)
  if (affectedApplied == null || affectedApplied.size > 0) return false

  if (!opts.allowUnusedPatches) {
    const currentGroups = groupsFromKeys(Object.keys(current))
    if (currentGroups == null) return false
    const applied = appliedPatchKeys(lockfile, currentGroups)
    if (applied == null) return false
    for (const key of allPatchKeys(currentGroups)) {
      if (!applied.has(key)) return false
    }
  }

  if (Object.keys(current).length === 0) {
    delete lockfile.patchedDependencies
  } else {
    lockfile.patchedDependencies = current
  }
  return true
}

function changedKeys (
  recorded: Record<string, string>,
  current: Record<string, string>
): string[] {
  const keys = new Set([...Object.keys(recorded), ...Object.keys(current)])
  return [...keys].filter((key) => recorded[key] !== current[key])
}

/**
 * Bucket `keys` the way the resolver buckets configured patches.
 *
 * Only the key decides what a patch matches, so this deliberately leaves the
 * payload empty rather than carrying hashes and paths it would never read.
 *
 * `undefined` for a key whose version segment is neither a version nor a
 * range, leaving `ERR_PNPM_PATCH_NON_SEMVER_RANGE` to the resolver.
 */
function groupsFromKeys (keys: string[]): PatchGroupRecord | undefined {
  try {
    return groupPatchedDependencies(
      Object.fromEntries(keys.map((key) => [key, { hash: '' }]))
    )
  } catch {
    return undefined
  }
}

/**
 * The patch keys in `patchGroups` that match a package the lockfile records, matched
 * the way the resolver matches them.
 *
 * `undefined` when a locked package matches more than one configured range, so
 * the caller falls back and lets the resolver raise
 * `ERR_PNPM_PATCH_KEY_CONFLICT` instead of quietly picking a winner.
 */
function appliedPatchKeys (
  lockfile: LockfileObject,
  patchGroups: PatchGroupRecord
): Set<string> | undefined {
  const applied = new Set<string>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    if (version == null) continue
    let patch
    try {
      patch = getPatchInfo(patchGroups, name, version)
    } catch {
      return undefined
    }
    if (patch != null) applied.add(patch.key)
  }
  return applied
}
