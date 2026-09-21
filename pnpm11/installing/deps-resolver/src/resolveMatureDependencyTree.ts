import * as dp from '@pnpm/deps.path'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from '@pnpm/resolving.npm-resolver'
import type { BlockedVersions, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { PkgResolutionId } from '@pnpm/types'

import type { ResolvedPkgsById } from './resolveDependencies.js'
import {
  type ImporterToResolveGeneric,
  type ResolveDependenciesOptions,
  resolveDependencyTree,
  type ResolveDependencyTreeResult,
} from './resolveDependencyTree.js'

/**
 * Upper bound on resolution passes.
 *
 * The loop already terminates on its own — every pass blocks at least one
 * more version, over a finite set — but "finite" is not "small": a package
 * whose every version in range pins something too young would be walked one
 * version per pass, and each pass is a full tree resolution. The bound is
 * what stops that from running for minutes.
 *
 * It is set well above the depth any real dependency chain reaches, since
 * blame only climbs one ancestor per pass and a tree deep enough to need
 * more has an unusual number of consecutive exact pins. Hitting it is
 * reported rather than passed over silently — the install then answers with
 * the first pass, and the user has no other way to tell that a later attempt
 * might have found a tree.
 */
const MAX_RESOLUTION_PASSES = 32

export interface MatureDependencyTreeResult<Importer> {
  importers: Importer[]
  tree: ResolveDependencyTreeResult
}

/**
 * Returns a tree with no minimumReleaseAge violations when bounded parent-version
 * retries find one. Other policy violations remain for their policy handlers.
 * Returns the original tree when the age conflict cannot be resolved.
 * toImporters must provide fresh inputs because resolution mutates specifiers.
 */
export async function resolveMatureDependencyTree<Importer extends ImporterToResolveGeneric<unknown>> (
  toImporters: () => Promise<Importer[]>,
  opts: ResolveDependenciesOptions
): Promise<MatureDependencyTreeResult<Importer>> {
  const runPass = async (blockedVersions?: BlockedVersions): Promise<MatureDependencyTreeResult<Importer>> => {
    // Rebuilt per pass: resolution rewrites the bare specifiers of the wanted
    // dependencies it walks, so reusing one pass's importers in the next would
    // carry the abandoned pass's pins into it.
    const importers = await toImporters()
    return { importers, tree: await resolveDependencyTree(importers, { ...opts, blockedVersions }) }
  }

  const firstPass = await runPass()
  if (!opts.minimumReleaseAge || !hasMaturityViolations(firstPass.tree)) {
    return firstPass
  }

  const blockedVersions = new Map<string, Set<string>>()
  let lastPass = firstPass
  for (let pass = 1; pass < MAX_RESOLUTION_PASSES; pass++) {
    if (!blockDeadEndParents(lastPass.tree, blockedVersions)) return firstPass
    // eslint-disable-next-line no-await-in-loop
    lastPass = await runPass(blockedVersions)
    if (!hasMaturityViolations(lastPass.tree)) {
      reportHeldBackParents(blockedVersions, lastPass.tree.resolvedPkgsById)
      return lastPass
    }
  }
  // Fell out of the loop with ancestors still left to try, so the report
  // below is the first pass's, not a proof that no installable tree exists.
  globalWarn(
    `Stopped after ${MAX_RESOLUTION_PASSES} resolution attempts while backing off from versions whose ` +
    'dependencies do not satisfy minimumReleaseAge. The versions reported are the ones the first attempt ' +
    'resolved to; an installable combination may still exist further down their ranges.'
  )
  return firstPass
}

/**
 * Records the immediate parent of every immature pick as unusable, and
 * reports whether another pass could still reach a clean tree.
 *
 * It cannot when a violation has no parent to blame: the importer named that
 * package itself, and no ancestor's choice can widen a range the manifest
 * fixes. Retrying past one of those only re-reaches the same failure, so the
 * install stops here and lets the policy handler act on this pass. It cannot
 * either when every parent to blame is already blocked, which means the walk
 * has run out of ancestors to move.
 */
function blockDeadEndParents (
  tree: ResolveDependencyTreeResult,
  blockedVersions: Map<string, Set<string>>
): boolean {
  let grew = false
  for (const violation of tree.resolutionPolicyViolations) {
    if (violation.code !== MINIMUM_RELEASE_AGE_VIOLATION_CODE) continue
    const parent = blamedParent(violation, tree.resolvedPkgsById)
    if (parent == null) return false
    let blockedForPkg = blockedVersions.get(parent.name)
    if (blockedForPkg == null) {
      blockedForPkg = new Set()
      blockedVersions.set(parent.name, blockedForPkg)
    }
    if (blockedForPkg.has(parent.version)) continue
    blockedForPkg.add(parent.version)
    grew = true
  }
  return grew
}

/**
 * The package whose choice of version put this pick in the tree: the last
 * entry of the package-only resolution path.
 */
function blamedParent (
  violation: ResolutionPolicyViolation,
  resolvedPkgsById: ResolvedPkgsById
): { name: string, version: string } | undefined {
  const parentIds = violation.parentIds
  // Empty means the importer asked for this package itself, and no
  // ancestor's choice can widen a range the manifest fixes.
  if (parentIds == null || parentIds.length === 0) return undefined
  const parentId = parentIds[parentIds.length - 1] as PkgResolutionId
  const parent = resolvedPkgsById[parentId]
  if (parent == null) return undefined
  const { registryName, version } = dp.parse(parent.id)
  if (version == null) return undefined
  return { name: parent.name, version: registryName ? `${registryName}:${version}` : version }
}

function reportHeldBackParents (
  blockedVersions: ReadonlyMap<string, ReadonlySet<string>>,
  resolvedPkgsById: ResolvedPkgsById
): void {
  const resolvedVersionsByName = new Map<string, Set<string>>()
  for (const { name, version } of Object.values(resolvedPkgsById)) {
    let versions = resolvedVersionsByName.get(name)
    if (versions == null) {
      versions = new Set()
      resolvedVersionsByName.set(name, versions)
    }
    versions.add(version)
  }
  // Sorted so the report reads the same across runs and matches pacquet's,
  // which sorts because its blocklist is keyed by hash. A Map preserves the
  // order versions happened to be blocked in, which is resolution order.
  const lines: string[] = []
  for (const [name, versions] of [...blockedVersions].sort(([left], [right]) => left.localeCompare(right))) {
    const resolvedTo = [...(resolvedVersionsByName.get(name) ?? [])].sort()
    for (const version of [...versions].sort()) {
      lines.push(
        `  ${name}@${version}` +
        (resolvedTo.length > 0 ? ` (resolved to ${resolvedTo.join(', ')} instead)` : '')
      )
    }
  }
  if (lines.length === 0) return
  globalInfo(
    'minimumReleaseAge held back the following versions because a package they depend on ' +
    `is younger than the cutoff:\n${lines.join('\n')}`
  )
}

function hasMaturityViolations (tree: ResolveDependencyTreeResult): boolean {
  return tree.resolutionPolicyViolations.some(violation => violation.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE)
}
