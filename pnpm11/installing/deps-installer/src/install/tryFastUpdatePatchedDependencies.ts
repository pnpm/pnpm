import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { allPatchKeys, getPatchInfo, type PatchGroupRecord } from '@pnpm/patching.config'

export interface FastPatchedDependenciesUpdateOptions {
  patchGroups: PatchGroupRecord | undefined
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
  const affected = changedKeys(recorded, current)
  if (affected.length === 0) return false

  const applied = appliedPatchKeys(lockfile, opts.patchGroups)
  if (applied == null) return false
  if (affected.some((key) => applied.has(key))) return false
  if (!opts.allowUnusedPatches && opts.patchGroups != null) {
    for (const key of allPatchKeys(opts.patchGroups)) {
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
 * The configured patch keys that match a package the lockfile records, matched
 * the way the resolver matches them.
 *
 * `undefined` when a locked package matches more than one configured range, so
 * the caller falls back and lets the resolver raise
 * `ERR_PNPM_PATCH_KEY_CONFLICT` instead of quietly picking a winner.
 */
function appliedPatchKeys (
  lockfile: LockfileObject,
  patchGroups: PatchGroupRecord | undefined
): Set<string> | undefined {
  const applied = new Set<string>()
  if (patchGroups == null) return applied
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
