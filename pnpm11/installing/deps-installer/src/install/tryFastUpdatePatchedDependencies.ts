import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import {
  allPatchKeys,
  getPatchInfo,
  groupPatchedDependencies,
  type PatchGroupRecord,
  verifyPatches,
} from '@pnpm/patching.config'
import type { DepPath } from '@pnpm/types'

export interface FastPatchedDependenciesUpdateOptions {
  patchedDependencies: Record<string, string> | undefined
  allowUnusedPatches: boolean
}

/** Where a package's key moves to when its patch changes. */
type Rekeys = Map<DepPath, DepPath>

/**
 * Absorb a changed `patchedDependencies` without resolving the dependency
 * graph.
 *
 * Resolution never reads a patch: it appends the patch file's hash to an
 * already-resolved package id, so the set of packages and versions is the same
 * either way. Only the affected packages' keys and the references pointing at
 * them move, which is a rewrite of the loaded lockfile rather than a
 * re-resolve.
 *
 * Returns `false` — nothing changed, a patched package is reachable as a peer,
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
  if (changedKeys(recorded, current).length === 0) return false

  const groups = groupsFromHashes(current)
  if (groups == null) return false
  if (!everyConfiguredPatchIsApplied(lockfile, opts)) return false

  const rekeys = planRekeys(lockfile, groups)
  if (rekeys == null) return false
  applyRekeys(lockfile, rekeys)

  if (Object.keys(current).length === 0) {
    delete lockfile.patchedDependencies
  } else {
    lockfile.patchedDependencies = current
  }
  return true
}

/**
 * Whether every configured patch still has a package to apply to.
 *
 * A patch with none left is `ERR_PNPM_UNUSED_PATCH`, which only a resolution
 * raises — `verifyPatches` runs from the resolver — so a rewrite that would
 * produce one has to decline and let the resolver report it. Any handler that
 * drops an edge can produce one, not only a changed patch configuration.
 *
 * `allowUnusedPatches` turns that error into a warning the resolution emits,
 * which no rewrite reproduces either; as elsewhere in this file, a warning is
 * not worth a full resolution.
 */
export function everyConfiguredPatchIsApplied (
  lockfile: LockfileObject,
  opts: FastPatchedDependenciesUpdateOptions
): boolean {
  if (opts.allowUnusedPatches) return true
  const configured = opts.patchedDependencies ?? {}
  if (Object.keys(configured).length === 0) return true
  const groups = groupsFromHashes(configured)
  if (groups == null) return false
  const applied = appliedPatchKeys(lockfile, groups)
  if (applied == null) return false
  for (const key of allPatchKeys(groups)) {
    if (!applied.has(key)) return false
  }
  return true
}

/**
 * Where every package key moves to once `groups` is the configured set of
 * patches: the same `name@version` and peer suffix, carrying the
 * `(patch_hash=...)` segment the new configuration gives it.
 *
 * `undefined` when the move cannot be made from lockfile data alone: a rekeyed
 * package that some snapshot reaches as a peer would rekey its dependents too
 * (their peer suffix embeds its depPath), and a peer suffix that pnpm
 * shortened into a hash cannot be inspected for that at all.
 */
function planRekeys (lockfile: LockfileObject, groups: PatchGroupRecord): Rekeys | undefined {
  const rekeys: Rekeys = new Map()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    const { name, version, nonSemverVersion, registryName } = nameVerFromPkgSnapshot(depPath, snapshot)
    const { patchHashIndex } = dp.indexOfDepPathSuffix(depPath)
    // The resolver matches patches against a package's plain semver version.
    // A named registry or a git / tarball reference occupies the same slot
    // here, and matching those cannot be reproduced from the key, so the
    // question is only whether it could matter: any configured patch naming
    // this package, or a patch hash already on the key, hands the decision
    // back to the resolver.
    if (version == null || nonSemverVersion != null || registryName != null) {
      if (groups[name] != null || patchHashIndex !== -1) return undefined
      continue
    }
    let patch
    try {
      patch = getPatchInfo(groups, name, version)
    } catch {
      return undefined
    }
    const base = dp.removeSuffix(depPath)
    const { peersIndex } = dp.indexOfDepPathSuffix(depPath)
    const peers = peersIndex === -1 ? '' : depPath.substring(peersIndex)
    const segment = patch ? `(patch_hash=${patch.hash})` : ''
    const moved = `${base}${segment}${peers}` as DepPath
    if (moved !== depPath) rekeys.set(depPath, moved)
  }
  if (rekeys.size === 0) return rekeys

  const movedBases = [...rekeys.keys()].map((depPath) => dp.removeSuffix(depPath))
  for (const depPath of Object.keys(lockfile.packages ?? {})) {
    const { peersIndex } = dp.indexOfDepPathSuffix(depPath)
    if (peersIndex === -1) continue
    const peers = depPath.substring(peersIndex)
    if (peerSuffixIsOpaque(peers) || movedBases.some((base) => peers.includes(base))) {
      return undefined
    }
  }
  return rekeys
}

/**
 * Whether `peers` is the short hash pnpm substitutes once the joined peer
 * segments exceed `peersSuffixMaxLength`, which hides the peers this rewrite
 * has to check for.
 */
function peerSuffixIsOpaque (peers: string): boolean {
  return peers
    .replace(/^\(/, '')
    .replace(/\)$/, '')
    .split(')(')
    .some((segment) => !segment.includes('@'))
}

function applyRekeys (lockfile: LockfileObject, rekeys: Rekeys): void {
  if (rekeys.size === 0) return
  const packages = lockfile.packages ?? {}
  for (const snapshot of Object.values(packages)) {
    rewriteReferences(snapshot.dependencies, rekeys)
    rewriteReferences(snapshot.optionalDependencies, rekeys)
  }
  lockfile.packages = Object.fromEntries(
    Object.entries(packages).map(([depPath, snapshot]) =>
      [rekeys.get(depPath as DepPath) ?? depPath, snapshot]
    )
  ) as typeof lockfile.packages
  for (const importer of Object.values(lockfile.importers)) {
    rewriteReferences(importer.dependencies, rekeys)
    rewriteReferences(importer.devDependencies, rekeys)
    rewriteReferences(importer.optionalDependencies, rekeys)
  }
}

function rewriteReferences (references: ResolvedDependencies | undefined, rekeys: Rekeys): void {
  if (references == null) return
  for (const [alias, reference] of Object.entries(references)) {
    const depPath = dp.refToRelative(reference, alias)
    const moved = depPath == null ? undefined : rekeys.get(depPath)
    if (moved == null) continue
    // A reference that already spelled out the whole depPath keeps doing so;
    // the bare-version shape keeps dropping the `<alias>@` prefix.
    references[alias] = depPath === reference ? moved : moved.substring(`${alias}@`.length)
  }
}

function changedKeys (
  recorded: Record<string, string>,
  current: Record<string, string>
): string[] {
  const keys = new Set([...Object.keys(recorded), ...Object.keys(current)])
  return [...keys].filter((key) => recorded[key] !== current[key])
}

/**
 * Report the patches the committed lockfile leaves unused, the way a
 * resolution reports them.
 *
 * Only reachable with `allowUnusedPatches` on: with it off,
 * `everyConfiguredPatchIsApplied` declines the rewrite instead, and the
 * resolution that takes over raises `ERR_PNPM_UNUSED_PATCH`.
 */
export function warnUnusedPatches (
  lockfile: LockfileObject,
  opts: FastPatchedDependenciesUpdateOptions
): void {
  if (!opts.allowUnusedPatches) return
  const configured = opts.patchedDependencies ?? {}
  if (Object.keys(configured).length === 0) return
  const groups = groupsFromHashes(configured)
  if (groups == null) return
  const applied = appliedPatchKeys(lockfile, groups)
  if (applied == null) return
  verifyPatches({ patchedDependencies: groups, appliedPatches: applied, allowUnusedPatches: true })
}

/**
 * Bucket `hashes` the way the resolver buckets configured patches.
 *
 * The patch file path is left out: nothing here applies a patch, and the
 * hashes `calcPatchHashes` already computed are the only payload the rewrite
 * needs.
 *
 * `undefined` for a key whose version segment is neither a version nor a
 * range, leaving `ERR_PNPM_PATCH_NON_SEMVER_RANGE` to the resolver.
 */
function groupsFromHashes (hashes: Record<string, string>): PatchGroupRecord | undefined {
  try {
    return groupPatchedDependencies(
      Object.fromEntries(Object.entries(hashes).map(([key, hash]) => [key, { hash }]))
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
