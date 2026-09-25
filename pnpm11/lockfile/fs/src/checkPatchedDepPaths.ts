import util from 'node:util'

import { parse, refToRelative } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, PackageSnapshots, ResolvedDependencies } from '@pnpm/lockfile.types'
import { getPatchInfo, groupPatchedDependencies } from '@pnpm/patching.config'
import type { PatchGroup, PatchGroupRecord } from '@pnpm/patching.types'
import type { DepPath } from '@pnpm/types'

export type PatchedDepPathsStatus =
  /** Every dependency path carries the suffix `patchedDependencies` gives its package, or none when no patch applies. */
  | 'up-to-date'
  /** At least one dependency path disagrees with `patchedDependencies`. */
  | 'stale'
  /** No dependency path was shown to disagree, but at least one could not be judged. */
  | 'indeterminate'

const PATCH_HASH_PREFIX = '(patch_hash='

/**
 * A patched dependency's hash is recorded in a lockfile multiple times.
 * 1) In the `patchedDependencies` map, which is the authoritative source of truth.
 * n) As a `(patch_hash=...)` suffix on every reference to patched packages (importers,
 *    snapshot keys and dependency edges).
 *
 * A dependency path missing the suffix its patch calls for disagrees as much as one carrying the
 * wrong hash.
 *
 * Judging a dependency path needs the package's version, which comes off its `packages` entry,
 * and the patch set, which comes off `patchedDependencies`. Either can be missing or malformed in
 * a lockfile that was hand-edited or merged badly, and neither failing is a patch-hash problem:
 * `'indeterminate'` keeps those apart from a hash that genuinely disagrees, so a caller can
 * re-resolve without reporting a cause it hasn't established. A `patchedDependencies` key that
 * does not resolve leaves only its own package without a verdict.
 *
 * Stops at the first dependency path that definitely disagrees.
 */
export function checkPatchedDepPaths (lockfile: LockfileObject): PatchedDepPathsStatus {
  const ctx = createJudgeContext(lockfile)
  let indeterminate = false
  for (const depPath of depPathsToJudge(lockfile, ctx)) {
    switch (judgeWithPeers(depPath, ctx)) {
      case 'stale': return 'stale'
      case 'indeterminate': indeterminate = true; break
      case 'ok': break
    }
  }
  return indeterminate ? 'indeterminate' : 'up-to-date'
}

type Verdict = 'ok' | 'stale' | 'indeterminate'

interface JudgeContext {
  patchGroups: PatchGroupRecord
  /**
   * Packages with a `patchedDependencies` key that does not resolve to a patch set. The resolver
   * reports the key itself, against the configured patches, where the one the user can act on
   * lives.
   */
  unusableNames: Set<string>
  /**
   * Every package named in `patchedDependencies`. A dependency path for any other package is
   * judged only when it carries a suffix.
   */
  patchedNames: Set<string>
  /**
   * Whether a peer segment is the peer's whole dependency path, and so carries the peer's own
   * patch hash. With `dedupePeers` it is only the peer's `name@version`.
   */
  peersCarryPatchHashes: boolean
  packages: PackageSnapshots
  verdicts: Map<DepPath, Verdict>
}

function createJudgeContext (lockfile: LockfileObject): JudgeContext {
  const usable: Record<string, string> = Object.create(null)
  const unusableNames = new Set<string>()
  for (const [key, hash] of Object.entries(lockfile.patchedDependencies ?? {})) {
    try {
      groupPatchedDependencies({ [key]: hash })
      usable[key] = hash
    } catch (err: unknown) {
      if (!isUnusablePatchConfig(err)) throw err
      unusableNames.add(parse(key).name ?? key)
    }
  }
  const patchGroups = groupPatchedDependencies(usable)
  return {
    patchGroups,
    unusableNames,
    patchedNames: new Set([...Object.keys(patchGroups), ...unusableNames]),
    peersCarryPatchHashes: lockfile.settings?.dedupePeers !== true,
    packages: lockfile.packages ?? {},
    verdicts: new Map(),
  }
}

/**
 * The worst verdict among the dependency path and the peer paths nested in its suffix, cached in
 * `ctx.verdicts`.
 *
 * pnpm writes a package's own hash as the first segment of the suffix. Unless peers are deduped,
 * a peer segment is that peer's whole dependency path, so a patched peer carries its hash inside
 * it, and that hash is part of this path's identity. A peer segment is judged when it carries a
 * marker, or when it should: it names a registry version of a patched package, the path itself is
 * a registry version, and peers are not deduped.
 *
 * A path holding a marker is `'indeterminate'` when the marker is anywhere but the leading segment
 * of its suffix, which is the trailing run of balanced, back-to-back parenthesized segments that
 * `parse` reads. Peers are walked level by level, so the depth a lockfile chooses never reaches
 * the call stack.
 */
function judgeWithPeers (depPath: DepPath, ctx: JudgeContext): Verdict {
  const cached = ctx.verdicts.get(depPath)
  if (cached != null) return cached
  let verdict: Verdict = 'ok'
  let level = [depPath]
  while (level.length > 0 && verdict !== 'stale') {
    const nextLevel: DepPath[] = []
    for (const path of level) {
      const judged = judgeOwnHash(path, ctx)
      verdict = worseVerdict(verdict, judged.verdict)
      for (const peer of judged.peers) nextLevel.push(peer)
    }
    level = nextLevel
  }
  ctx.verdicts.set(depPath, verdict)
  return verdict
}

/** The verdict on the path's own hash, and the peer paths in its suffix that carry one. */
function judgeOwnHash (depPath: DepPath, ctx: JudgeContext): { verdict: Verdict, peers: DepPath[] } {
  const suffix = splitSuffix(depPath)
  if (
    depPath.includes(PATCH_HASH_PREFIX) && (
      suffix == null ||
      suffix.locator.includes(PATCH_HASH_PREFIX) ||
      suffix.segments.slice(1).some((segment) => segment.startsWith(PATCH_HASH_PREFIX))
    )
  ) {
    return { verdict: 'indeterminate', peers: [] }
  }
  // Only a registry version is known to end before the suffix. A `file:` or other locator can end
  // in text shaped like a peer segment, so a peer without a hash is not inferred there.
  const infersUnmarkedPeers = ctx.peersCarryPatchHashes && parse(depPath).version != null
  const peers: DepPath[] = []
  for (const segment of suffix?.segments ?? []) {
    if (segment.startsWith(PATCH_HASH_PREFIX)) continue
    const peer = segment.slice(1, -1) as DepPath
    if (segment.includes(PATCH_HASH_PREFIX) || (infersUnmarkedPeers && isPatchedRegistryPeer(peer, ctx))) {
      peers.push(peer)
    }
  }
  return { verdict: judge(depPath, ctx), peers }
}

/**
 * Whether a peer segment names a registry version of a patched package, so it has to carry that
 * package's hash. A `link:` peer is written with a path where the version goes, and patches never
 * apply to it.
 */
function isPatchedRegistryPeer (peer: DepPath, ctx: JudgeContext): boolean {
  const { name, version } = parse(peer)
  return name != null && version != null && ctx.patchedNames.has(name)
}

function worseVerdict (a: Verdict, b: Verdict): Verdict {
  if (a === 'stale' || b === 'stale') return 'stale'
  return a === 'indeterminate' || b === 'indeterminate' ? 'indeterminate' : 'ok'
}

/**
 * The trailing run of balanced, back-to-back parenthesized segments that ends the dependency
 * path, read right to left as `parse` reads it, and the locator text in front of it. A locator
 * such as a `file:` path can hold parentheses of its own. `undefined` when a segment's opening
 * parenthesis is missing.
 */
function splitSuffix (depPath: string): { locator: string, segments: string[] } | undefined {
  const segments: string[] = []
  let end = depPath.length
  while (end > 0 && depPath[end - 1] === ')') {
    let depth = 0
    let start = end - 1
    for (; start >= 0; start--) {
      if (depPath[start] === ')') {
        depth++
      } else if (depPath[start] === '(') {
        depth--
        if (depth === 0) break
      }
    }
    if (start < 0) return undefined
    segments.push(depPath.slice(start, end))
    end = start
  }
  return { locator: depPath.slice(0, end), segments: segments.reverse() }
}

function judge (depPath: DepPath, ctx: JudgeContext): Verdict {
  const parsed = parse(depPath)
  const { name } = parsed
  if (name == null || ctx.unusableNames.has(name)) return 'indeterminate'
  // The entry can be absent: rewriting only some of a package's `(patch_hash=...)` occurrences
  // leaves the rest pointing at a snapshot key that no longer exists. A non-registry package's
  // version slot holds its resolution instead, so the version a patch was matched against is only
  // the one its entry records. Without that, a key that selects on version cannot be matched
  // either way.
  const version = ctx.packages[depPath]?.version ?? parsed.version
  if (version == null && patchSelectsOnVersion(ctx.patchGroups[name])) return 'indeterminate'
  const recordedHash = parsed.patchHash?.slice(PATCH_HASH_PREFIX.length, -1)
  try {
    return getPatchInfo(ctx.patchGroups, name, version ?? parsed.nonSemverVersion ?? '')?.hash === recordedHash
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

/** Every dependency path in the lockfile that either carries a patch-hash marker or names a patched package. */
function * depPathsToJudge (lockfile: LockfileObject, ctx: JudgeContext): Generator<DepPath> {
  for (const importer of Object.values(lockfile.importers ?? {})) {
    yield * referencesToJudge(importer.dependencies, ctx)
    yield * referencesToJudge(importer.devDependencies, ctx)
    yield * referencesToJudge(importer.optionalDependencies, ctx)
  }
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (mayBePatched(depPath, ctx)) yield depPath
    yield * referencesToJudge(snapshot.dependencies, ctx)
    yield * referencesToJudge(snapshot.optionalDependencies, ctx)
  }
}

function * referencesToJudge (deps: ResolvedDependencies | undefined, ctx: JudgeContext): Generator<DepPath> {
  if (deps == null) return
  for (const [alias, reference] of Object.entries(deps)) {
    // A reference without an `@` points at a package named after its alias, so most edges are
    // skipped here without building their dependency path.
    if (!reference.includes('@') && !ctx.patchedNames.has(alias) && !reference.includes(PATCH_HASH_PREFIX)) continue
    const depPath = refToRelative(reference, alias)
    if (depPath != null && mayBePatched(depPath, ctx)) yield depPath
  }
}

function mayBePatched (depPath: DepPath, ctx: JudgeContext): boolean {
  return depPath.includes(PATCH_HASH_PREFIX) ||
    ctx.patchedNames.has(depPath.slice(0, depPath.indexOf('@', 1))) ||
    (ctx.peersCarryPatchHashes && namesAPatchedPeer(depPath, ctx.patchedNames))
}

function namesAPatchedPeer (depPath: DepPath, patchedNames: Set<string>): boolean {
  if (!depPath.includes('(')) return false
  for (const name of patchedNames) {
    if (depPath.includes(`(${name}@`)) return true
  }
  return false
}
