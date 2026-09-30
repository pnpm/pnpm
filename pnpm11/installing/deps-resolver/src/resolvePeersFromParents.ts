import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type {
  ParentPackages,
  PeerDependencyIssues,
  ProjectRootDir,
} from '@pnpm/types'
import * as semverUtils from '@yarnpkg/core/semverUtils'
import { pick } from 'ramda'

import type { NodeId } from './nextNodeId.js'
import type {
  MissingPeerInfo,
  ParentRef,
  ParentRefs,
  PartialResolvedPackage,
  PeersResolution,
} from './peersResolutionTypes.js'
import type { DependenciesTree, ResolvedPackage } from './resolveDependencies.js'

type PeerIssues = Pick<PeerDependencyIssues, 'bad' | 'missing'>

interface ResolvePeersFromParentsContext<Pkg extends PartialResolvedPackage> {
  currentDepth: number
  lockfileDir: string
  nodeId: NodeId
  parentPkgs: ParentRefs
  parentNodeIds: NodeId[]
  resolvedPackage: Pkg
  dependenciesTree: DependenciesTree<Pkg>
  rootDir: ProjectRootDir
  peerDependencyIssues: PeerIssues
}

export function resolvePeersFromParents<Pkg extends PartialResolvedPackage> (
  ctx: ResolvePeersFromParentsContext<Pkg>
): PeersResolution {
  const resolvedPeers = new Map<string, NodeId>()
  const missingPeers = new Map<string, MissingPeerInfo>()
  for (const [peerName, { version, optional }] of Object.entries(ctx.resolvedPackage.peerDependencies)) {
    const peerVersionRange = getPeerVersionRange(version)

    const resolved = ctx.parentPkgs[peerName]
    const optionalPeer = optional === true

    if (!resolved) {
      missingPeers.set(peerName, { range: version, optional: optionalPeer })
      reportMissingPeer(ctx, peerName, { optional: optionalPeer, wantedRange: peerVersionRange })
      continue
    }

    if (!semverUtils.satisfiesWithPrereleases(resolved.version, peerVersionRange, true)) {
      reportBadPeer(ctx, { peerName, resolved, optional: optionalPeer, wantedRange: peerVersionRange })
    }

    if (resolved?.nodeId) resolvedPeers.set(peerName, resolved.nodeId)
  }
  return { resolvedPeers, missingPeers }
}

function reportMissingPeer<Pkg> (
  ctx: {
    dependenciesTree: DependenciesTree<Pkg>
    parentNodeIds: NodeId[]
    peerDependencyIssues: PeerIssues
  },
  peerName: string,
  { optional, wantedRange }: { optional: boolean, wantedRange: string }
): void {
  const location = getLocationFromParentNodeIds(ctx)
  if (!ctx.peerDependencyIssues.missing[peerName]) {
    ctx.peerDependencyIssues.missing[peerName] = []
  }
  ctx.peerDependencyIssues.missing[peerName].push({
    parents: location.parents,
    optional,
    wantedRange,
  })
}

function reportBadPeer<Pkg extends PartialResolvedPackage> (
  ctx: ResolvePeersFromParentsContext<Pkg>,
  opts: {
    peerName: string
    resolved: ParentRef
    optional: boolean
    wantedRange: string
  }
): void {
  const { peerName, resolved } = opts
  const location = getLocationFromParentNodeIds(ctx)
  if (!ctx.peerDependencyIssues.bad[peerName]) {
    ctx.peerDependencyIssues.bad[peerName] = []
  }
  const peerLocation = resolved.nodeId == null
    ? []
    : getLocationFromParentNodeIds({
      dependenciesTree: ctx.dependenciesTree,
      parentNodeIds: resolved.parentNodeIds,
    }).parents
  ctx.peerDependencyIssues.bad[peerName].push({
    foundVersion: resolved.version,
    resolvedFrom: peerLocation,
    parents: location.parents,
    optional: opts.optional,
    wantedRange: opts.wantedRange,
  })
}

interface Location {
  projectId: string
  parents: ParentPackages
}

export function getLocationFromParentNodeIds<Pkg> (
  {
    dependenciesTree,
    parentNodeIds,
  }: {
    dependenciesTree: DependenciesTree<Pkg>
    parentNodeIds: NodeId[]
  }
): Location {
  const parents = parentNodeIds
    .map((nid) => pick(['name', 'version'], dependenciesTree.get(nid)!.resolvedPackage as ResolvedPackage))
  return {
    projectId: '.',
    parents,
  }
}
