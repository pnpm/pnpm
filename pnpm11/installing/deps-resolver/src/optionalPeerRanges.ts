import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import * as semverUtils from '@yarnpkg/core/semverUtils'

import type { ParentRef, ParentRefs, PartialResolvedPackage } from './peersResolutionTypes.js'
import type { DependenciesTree } from './resolveDependencies.js'

/** Every optional peer range declared in the tree, by peer name. */
export type OptionalPeerRanges = Map<string, Set<string>>

export function collectOptionalPeerRanges<Pkg extends PartialResolvedPackage> (dependenciesTree: DependenciesTree<Pkg>): OptionalPeerRanges {
  const ranges: OptionalPeerRanges = new Map()
  for (const { resolvedPackage } of dependenciesTree.values()) {
    for (const [peerName, { version, optional }] of Object.entries((resolvedPackage as Pkg).peers?.taken ?? {})) {
      if (optional !== true) continue
      let peerRanges = ranges.get(peerName)
      if (peerRanges == null) {
        peerRanges = new Set()
        ranges.set(peerName, peerRanges)
      }
      peerRanges.add(getPeerVersionRange(version))
    }
  }
  return ranges
}

/**
 * The version of `provider` when it is restricted: a hoisted peer provider
 * that some optional peer range in the tree rejects. Only such a provider can
 * resolve a subtree differently from a declared one, so a cached peer
 * resolution is reused only under the restriction it was recorded under.
 */
export function restrictedProviderVersion (
  ranges: OptionalPeerRanges,
  peerName: string,
  provider: ParentRef | undefined
): string | undefined {
  if (provider?.hoistedPeerProvider !== true) return undefined
  for (const range of ranges.get(peerName) ?? []) {
    if (!semverUtils.satisfiesWithPrereleases(provider.version, range, true)) return provider.version
  }
  return undefined
}

/** The restricted providers among the `parentPkgs` of `peerNames`, by peer name. */
export function restrictedProviderVersions (
  ranges: OptionalPeerRanges,
  parentPkgs: ParentRefs,
  peerNames: string[]
): Map<string, string> {
  const versions = new Map<string, string>()
  for (const peerName of peerNames) {
    const version = Object.hasOwn(parentPkgs, peerName)
      ? restrictedProviderVersion(ranges, peerName, parentPkgs[peerName])
      : undefined
    if (version != null) versions.set(peerName, version)
  }
  return versions
}
