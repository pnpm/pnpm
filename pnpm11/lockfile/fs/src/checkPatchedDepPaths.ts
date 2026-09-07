import util from 'node:util'

import { parse, refToRelative } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, PackageSnapshots, ResolvedDependencies } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { getPatchInfo, groupPatchedDependencies } from '@pnpm/patching.config'
import type { PatchGroup, PatchGroupRecord } from '@pnpm/patching.types'
import type { DepPath } from '@pnpm/types'

export type PatchedDepPathsStatus =
  /** Every `(patch_hash=...)` suffix matches the patch `patchedDependencies` gives its package. */
  | 'up-to-date'
  /** At least one suffix disagrees with `patchedDependencies`. */
  | 'stale'
  /** No suffix was shown to disagree, but at least one could not be judged. */
  | 'indeterminate'

/**
 * A patched dependency's hash is recorded in a lockfile multiple times.
 * 1) In the `patchedDependencies` map, which is the authoritative source of truth.
 * n) As a `(patch_hash=...)` suffix on every reference to patched packages (importers,
 *    snapshot keys and dependency edges).
 *
 * Judging a suffix needs the package's version, which comes off its `packages` entry, and the
 * patch set, which comes off `patchedDependencies`. Either can be missing or malformed in a
 * lockfile that was hand-edited or merged badly, and neither failing is a patch-hash problem:
 * `'indeterminate'` keeps those apart from a hash that genuinely disagrees, so a caller can
 * re-resolve without reporting a cause it hasn't established.
 *
 * Stops at the first suffix that definitely disagrees.
 */
export function checkPatchedDepPaths (lockfile: LockfileObject): PatchedDepPathsStatus {
  let patchGroups: PatchGroupRecord
  try {
    patchGroups = groupPatchedDependencies(lockfile.patchedDependencies ?? {})
  } catch (err: unknown) {
    if (!isUnusablePatchConfig(err)) throw err
    return 'indeterminate'
  }
  const packages = lockfile.packages ?? {}
  let indeterminate = false
  for (const depPath of patchedDepPaths(lockfile, packages)) {
    switch (judge(depPath, patchGroups, packages)) {
      case 'stale': return 'stale'
      case 'indeterminate': indeterminate = true; break
      case 'ok': break
    }
  }
  return indeterminate ? 'indeterminate' : 'up-to-date'
}

type Verdict = 'ok' | 'stale' | 'indeterminate'

function judge (depPath: DepPath, patchGroups: PatchGroupRecord, packages: PackageSnapshots): Verdict {
  const { patchHash } = parse(depPath)
  if (patchHash == null) return 'ok'
  // The entry can be absent: rewriting only some of a package's `(patch_hash=...)` occurrences
  // leaves the rest pointing at a snapshot key that no longer exists. A registry package's
  // dependency path still carries its version, so those are judged anyway.
  const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath, packages[depPath])
  // A non-registry package's version slot holds its resolution instead, so the version a patch
  // was matched against is only the one its entry records. Without that, a key that selects on
  // version cannot be matched either way.
  if (version == null && patchSelectsOnVersion(patchGroups[name])) return 'indeterminate'
  const recordedHash = patchHash.slice('(patch_hash='.length, -1)
  try {
    return getPatchInfo(patchGroups, name, version ?? nonSemverVersion ?? '')?.hash === recordedHash
      ? 'ok'
      : 'stale'
  } catch (err: unknown) {
    if (!isUnusablePatchConfig(err)) throw err
    return 'indeterminate'
  }
}

/** Whether which patch applies for `group`, if any, can depend on the package's version. */
function patchSelectsOnVersion (group: PatchGroup | undefined): boolean {
  // No entry resolves to no patch for every version, and a bare-name entry to the same patch for
  // every version. Either way the version cannot change the answer.
  return group != null && (Object.keys(group.exact).length > 0 || group.range.length > 0)
}

/**
 * Whether `err` is the `patchedDependencies` map refusing to resolve to a patch set. The resolver
 * reports these against the configured patches, where the key the user can act on lives.
 */
function isUnusablePatchConfig (err: unknown): boolean {
  return util.types.isNativeError(err) &&
    'code' in err &&
    (err.code === 'ERR_PNPM_PATCH_NON_SEMVER_RANGE' || err.code === 'ERR_PNPM_PATCH_KEY_CONFLICT')
}

/** Every dependency path in the lockfile that carries a `(patch_hash=...)` suffix. */
function * patchedDepPaths (lockfile: LockfileObject, packages: PackageSnapshots): Generator<DepPath> {
  for (const importer of Object.values(lockfile.importers ?? {})) {
    yield * patchedReferences(importer.dependencies)
    yield * patchedReferences(importer.devDependencies)
    yield * patchedReferences(importer.optionalDependencies)
  }
  for (const [depPath, snapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    yield depPath
    yield * patchedReferences(snapshot.dependencies)
    yield * patchedReferences(snapshot.optionalDependencies)
  }
}

function * patchedReferences (deps: ResolvedDependencies | undefined): Generator<DepPath> {
  if (deps == null) return
  for (const [alias, reference] of Object.entries(deps)) {
    // Fast-path: typically most dependencies are not patched, faster to skip parsing them.
    if (!reference.includes('(patch_hash=')) continue
    const depPath = refToRelative(reference, alias)
    if (depPath != null) yield depPath
  }
}
