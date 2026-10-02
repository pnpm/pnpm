import type {
  DepPath,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
  PkgIdWithPatchHash,
} from '@pnpm/types'
import pDefer, { type DeferredPromise } from 'p-defer'

import { breakDepPathAwaitCycles } from './breakDepPathAwaitCycles.js'
import { dedupeInjectedDeps } from './dedupeInjectedDeps.js'
import { deduplicateAll } from './deduplicateDepPaths.js'
import { getNodeIdsByPreviousDepPath } from './lockedPeerProviders.js'
import { mergePeers } from './mergePeers.js'
import type { NodeId } from './nextNodeId.js'
import { createPkgsByName } from './parentRefs.js'
import type {
  CurrentProviderSource,
  DependenciesByProjectId,
  FinishingResolutionPromise,
  GenericDependenciesGraph,
  GenericDependenciesGraphWithResolvedChildren,
  ParentPkgsOfNode,
  ParentRefs,
  PartialResolvedPackage,
  ProjectToResolve,
  ResolveNodePeersContext,
} from './peersResolutionTypes.js'
import type { DependenciesTree } from './resolveDependencies.js'
import type { ResolvedImporters } from './resolveDependencyTree.js'
import { resolvePeersOfChildren } from './resolvePeersOfNode.js'

export type {
  BaseGenericDependenciesGraphNode,
  DependenciesByProjectId,
  GenericDependenciesGraph,
  GenericDependenciesGraphNode,
  GenericDependenciesGraphNodeWithResolvedChildren,
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
  ProjectToResolve,
} from './peersResolutionTypes.js'

interface ResolvePeersOptions<Pkg extends PartialResolvedPackage> {
  allPeerDepNames: Set<string>
  projects: ProjectToResolve[]
  dependenciesTree: DependenciesTree<Pkg>
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  lockfileDir: string
  resolvePeersFromWorkspaceRoot?: boolean
  dedupePeerDependents?: boolean
  dedupePeers?: boolean
  dedupeInjectedDeps?: boolean
  resolvedImporters: ResolvedImporters
  peersSuffixMaxLength: number
  workspaceProjectIds: Set<string>
  resolvedPeerProviderPaths?: Map<NodeId, DepPath>
}

type ProjectScopedContextKeys =
  | 'parentPkgsOfNode'
  | 'parentNodeIds'
  | 'parentDepPathsChain'
  | 'currentProviderSources'
  | 'peerDependencyIssues'
  | 'rootDir'

type SharedPeersContext<Pkg extends PartialResolvedPackage> = Omit<ResolveNodePeersContext<Pkg>, ProjectScopedContextKeys> & {
  depPathsByPkgId: Map<PkgIdWithPatchHash, Set<DepPath>>
}

type PeerIssuesOfProject = Pick<PeerDependencyIssues, 'bad' | 'missing'>

export async function resolvePeers<Pkg extends PartialResolvedPackage> (
  opts: ResolvePeersOptions<Pkg>
): Promise<{
  dependenciesGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
  dependenciesByProjectId: DependenciesByProjectId
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  pathsByNodeId: Map<NodeId, DepPath>
}> {
  const sharedCtx = createSharedPeersContext(opts)
  const workspaceRootProject = opts.resolvePeersFromWorkspaceRoot && opts.projects.length > 1
    ? opts.projects.find(({ id }) => id === '.')
    : undefined
  const rootPkgsByName = workspaceRootProject == null ? {} : createPkgsByName(opts.dependenciesTree, workspaceRootProject)
  const peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects = {}

  const finishingList: FinishingResolutionPromise[] = []
  for (const project of opts.projects) {
    // eslint-disable-next-line no-await-in-loop -- projects share the peers cache and parentPkgsOfNode, so they are resolved one at a time
    const projectResolution = await resolvePeersOfProject(sharedCtx, { project, workspaceRootProject, rootPkgsByName })
    finishingList.push(...projectResolution.finishingList)
    addPeerDependencyIssuesOfProject(peerDependencyIssuesByProjects, project.id, projectResolution.peerDependencyIssues)
  }
  breakDepPathAwaitCycles(sharedCtx)
  await Promise.all(finishingList)

  const depGraphWithResolvedChildren = resolveChildren(sharedCtx.depGraph, sharedCtx.pathsByNodeId)
  const dependenciesByProjectId = getDependenciesByProjectId(opts.projects, sharedCtx.pathsByNodeId)
  if (opts.dedupeInjectedDeps) {
    dedupeInjectedDeps({
      dependenciesByProjectId,
      projects: opts.projects,
      depGraph: depGraphWithResolvedChildren,
      pathsByNodeId: sharedCtx.pathsByNodeId,
      lockfileDir: opts.lockfileDir,
      resolvedImporters: opts.resolvedImporters,
      workspaceProjectIds: opts.workspaceProjectIds,
    })
  }
  if (opts.dedupePeerDependents) {
    dedupePeerDependents(depGraphWithResolvedChildren, { dependenciesByProjectId, depPathsByPkgId: sharedCtx.depPathsByPkgId, projects: opts.projects })
  }
  return {
    dependenciesGraph: depGraphWithResolvedChildren,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
    pathsByNodeId: sharedCtx.pathsByNodeId,
  }
}

function createSharedPeersContext<Pkg extends PartialResolvedPackage> (opts: ResolvePeersOptions<Pkg>): SharedPeersContext<Pkg> {
  return {
    allPeerDepNames: opts.allPeerDepNames,
    dependenciesTree: opts.dependenciesTree,
    depGraph: {},
    lockfileDir: opts.lockfileDir,
    pathsByNodeId: new Map<NodeId, DepPath>(),
    pathsByNodeIdPromises: new Map<NodeId, DeferredPromise<DepPath>>(),
    awaitedPeerNodeIdsByNodeId: new Map<NodeId, Set<NodeId>>(),
    peersCacheOwnerByNodeId: new Map<NodeId, NodeId>(),
    cycleBrokenNodeIds: new Set<NodeId>(),
    depPathsByPkgId: new Map<PkgIdWithPatchHash, Set<DepPath>>(),
    nodeIdsByPreviousDepPath: opts.resolvedPeerProviderPaths == null
      ? new Map<DepPath, NodeId>()
      : getNodeIdsByPreviousDepPath(opts.dependenciesTree),
    resolvedPeerProviderPaths: opts.resolvedPeerProviderPaths,
    peersCache: new Map(),
    purePkgs: new Set<PkgIdWithPatchHash>(),
    dedupePeers: opts.dedupePeers,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  }
}

async function resolvePeersOfProject<Pkg extends PartialResolvedPackage> (
  sharedCtx: SharedPeersContext<Pkg>,
  opts: {
    project: ProjectToResolve
    workspaceRootProject: ProjectToResolve | undefined
    rootPkgsByName: ParentRefs
  }
): Promise<{ finishingList: FinishingResolutionPromise[], peerDependencyIssues: PeerIssuesOfProject }> {
  const { project } = opts
  const currentProviderSources = getCurrentProviderSources(project, opts.workspaceRootProject)
  const peerDependencyIssues: PeerIssuesOfProject = { bad: {}, missing: {} }
  const pkgsByName = getPeerProvidersOfProject(sharedCtx, { project, rootPkgsByName: opts.rootPkgsByName })
  const { ownDirectChildren, hoistedProviderChildren } = splitHoistedProviderChildren(project)
  const parentPkgsOfNode: ParentPkgsOfNode = new Map()
  const projectPeersContext: ResolveNodePeersContext<Pkg> = {
    ...sharedCtx,
    parentPkgsOfNode,
    parentNodeIds: [],
    parentDepPathsChain: [],
    currentProviderSources,
    peerDependencyIssues,
    rootDir: project.rootDir,
  }
  const finishingList: FinishingResolutionPromise[] = [
    (await resolvePeersOfChildren(ownDirectChildren, pkgsByName, projectPeersContext)).finishing,
  ]
  // A provider whose tree position was pruned from the traversal (its parent
  // hit peersCache, so its children were never visited) still has consumers
  // awaiting its dep path, so resolve it here as a last resort. Providers
  // visited by the traversal above are recorded in parentPkgsOfNode.
  // All pruned providers go into a single resolvePeersOfChildren call:
  // its cycle analysis only sees the children of one call, and providers
  // frequently peer-depend on each other, so resolving them one by one
  // would leave their dep path calculations awaiting each other forever.
  // A peer cycle that spans this call and the traversal above is still
  // invisible here; breakDepPathAwaitCycles resolves those before the
  // finishing promises are awaited.
  const prunedProviderChildren: Record<string, NodeId> = {}
  for (const [alias, nodeId] of Object.entries(hoistedProviderChildren)) {
    if (parentPkgsOfNode.has(nodeId)) continue
    prunedProviderChildren[alias] = nodeId
  }
  if (Object.keys(prunedProviderChildren).length > 0) {
    finishingList.push((await resolvePeersOfChildren(prunedProviderChildren, pkgsByName, projectPeersContext)).finishing)
  }
  return { finishingList, peerDependencyIssues }
}

function getCurrentProviderSources (
  project: ProjectToResolve,
  workspaceRootProject: ProjectToResolve | undefined
): CurrentProviderSource[] {
  const currentProviderSources: CurrentProviderSource[] = [toCurrentProviderSource(project)]
  if (workspaceRootProject != null && workspaceRootProject.id !== project.id) {
    currentProviderSources.push(toCurrentProviderSource(workspaceRootProject))
  }
  return currentProviderSources
}

function toCurrentProviderSource (project: ProjectToResolve): CurrentProviderSource {
  return {
    directNodeIdsByAlias: project.directNodeIdsByAlias,
    declaredDirectDependencies: project.declaredDirectDependencies ?? new Set(),
    explicitlyRequestedDirectDependencies: project.explicitlyRequestedDirectDependencies ?? new Set(),
  }
}

function getPeerProvidersOfProject<Pkg extends PartialResolvedPackage> (
  sharedCtx: SharedPeersContext<Pkg>,
  opts: {
    project: ProjectToResolve
    rootPkgsByName: ParentRefs
  }
): ParentRefs {
  const pkgsByName = Object.fromEntries(Object.entries({
    ...opts.rootPkgsByName,
    ...createPkgsByName(sharedCtx.dependenciesTree, opts.project),
  }).filter(([peerName]) => sharedCtx.allPeerDepNames.has(peerName)))
  for (const { nodeId } of Object.values(pkgsByName)) {
    if (nodeId && !sharedCtx.pathsByNodeIdPromises.has(nodeId)) {
      sharedCtx.pathsByNodeIdPromises.set(nodeId, pDefer())
    }
  }
  return pkgsByName
}

// Hoisted peer providers stay visible as providers (via pkgsByName) but are
// not traversed as direct children: their nodeIds point into subtrees, and
// resolving them a second time in the project's root context would bind
// their peers to the project's own dependencies instead of the providers
// next to them in the tree, racing with the in-place resolution on
// pathsByNodeId and producing peer graphs that mix both contexts.
function splitHoistedProviderChildren (
  project: ProjectToResolve
): { ownDirectChildren: Record<string, NodeId>, hoistedProviderChildren: Record<string, NodeId> } {
  const ownDirectChildren: Record<string, NodeId> = {}
  const hoistedProviderChildren: Record<string, NodeId> = {}
  for (const [alias, nodeId] of project.directNodeIdsByAlias.entries()) {
    if (project.hoistedPeerProviderNodeIds?.has(nodeId)) {
      hoistedProviderChildren[alias] = nodeId
    } else {
      ownDirectChildren[alias] = nodeId
    }
  }
  return { ownDirectChildren, hoistedProviderChildren }
}

function addPeerDependencyIssuesOfProject (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  projectId: string,
  peerDependencyIssues: PeerIssuesOfProject
): void {
  if (Object.keys(peerDependencyIssues.bad).length > 0 || Object.keys(peerDependencyIssues.missing).length > 0) {
    peerDependencyIssuesByProjects[projectId] = {
      ...peerDependencyIssues,
      ...mergePeers(peerDependencyIssues.missing),
    }
  }
}

function resolveChildren<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraph<Pkg>,
  pathsByNodeId: Map<NodeId, DepPath>
): GenericDependenciesGraphWithResolvedChildren<Pkg> {
  for (const node of Object.values(depGraph)) {
    node.children = {}
    for (const [alias, childNodeId] of Object.entries<NodeId>(node.childrenNodeIds)) {
      node.children[alias] = pathsByNodeId.get(childNodeId) ?? (childNodeId as unknown as DepPath)
    }
    delete node.childrenNodeIds
  }
  return depGraph as unknown as GenericDependenciesGraphWithResolvedChildren<Pkg>
}

function getDependenciesByProjectId (
  projects: ProjectToResolve[],
  pathsByNodeId: Map<NodeId, DepPath>
): DependenciesByProjectId {
  const dependenciesByProjectId: DependenciesByProjectId = {}
  for (const { directNodeIdsByAlias, id } of projects) {
    dependenciesByProjectId[id] = new Map()
    for (const [alias, nodeId] of directNodeIdsByAlias.entries()) {
      dependenciesByProjectId[id].set(alias, pathsByNodeId.get(nodeId)!)
    }
  }
  return dependenciesByProjectId
}

function dedupePeerDependents<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  opts: {
    dependenciesByProjectId: DependenciesByProjectId
    depPathsByPkgId: Map<PkgIdWithPatchHash, Set<DepPath>>
    projects: ProjectToResolve[]
  }
): void {
  const { dependenciesByProjectId } = opts
  const duplicates = Array.from(opts.depPathsByPkgId.values()).filter((item) => item.size > 1)
  const allDepPathsMap = deduplicateAll(depGraph, duplicates)
  for (const { id } of opts.projects) {
    for (const [alias, depPath] of dependenciesByProjectId[id].entries()) {
      dependenciesByProjectId[id].set(alias, allDepPathsMap[depPath] ?? depPath)
    }
  }
}
