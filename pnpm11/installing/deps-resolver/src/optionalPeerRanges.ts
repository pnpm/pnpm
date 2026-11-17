import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import * as semverUtils from '@yarnpkg/core/semverUtils'

import type { PartialResolvedPackage } from './peersResolutionTypes.js'
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

/** Whether some optional peer range declared for `peerName` rejects `version`. */
export function isOutsideAnOptionalPeerRange (ranges: OptionalPeerRanges, peerName: string, version: string): boolean {
  for (const range of ranges.get(peerName) ?? []) {
    if (!semverUtils.satisfiesWithPrereleases(version, range, true)) return true
  }
  return false
}
