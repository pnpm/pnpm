import path from 'node:path'

import { linkBinsOfPkgsByAliases, type WarnFunction } from '@pnpm/bins.linker'
import { createMatcher } from '@pnpm/config.matcher'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { logger } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { DepPath, HoistedDependencies, ProjectId } from '@pnpm/types'
import { isSubdir } from 'is-subdir'

import { createGetAliasHoistType, type GetAliasHoistType } from './createGetAliasHoistType.js'
import { graphWalker, type GraphWalkerStep } from './graphWalker.js'
import { hoistLogger, selectHoistedModulesDir } from './hoistedModulesDirs.js'
import { hoistWorkspacePackages, type HoistWorkspacePackagesOpts } from './hoistWorkspacePackages.js'
import { symlinkHoistedDependency } from './symlinkHoistedDependency.js'
import type {
  DependenciesGraph,
  DependenciesGraphNode,
  DirectDependenciesByImporterId,
  HoistedDependenciesByNodeId,
  HoistedWorkspaceProject,
  HoistGraphResult,
  HoistType,
} from './types.js'
import { collectOccupiedAliases } from './workspaceAliasConflicts.js'

export { type GraphDependency, type GraphWalker, graphWalker, type GraphWalkerStep } from './graphWalker.js'
export { hoistWorkspacePackages, type HoistWorkspacePackagesOpts, pruneStaleWorkspaceHoists } from './hoistWorkspacePackages.js'
export type { DependenciesGraph, DependenciesGraphNode, DirectDependenciesByImporterId, HoistedWorkspaceProject } from './types.js'

export interface HoistOpts<NodeId extends string> extends GetHoistedDependenciesOpts<NodeId> {
  beforeWorkspaceLinks?: HoistWorkspacePackagesOpts<NodeId>['beforeWorkspaceLinks']
  directDependencyAliases?: HoistWorkspacePackagesOpts<NodeId>['directDependencyAliases']
  extraNodePath?: string[]
  preferSymlinkedExecutables?: boolean
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
}

export async function hoist<NodeId extends string> (opts: HoistOpts<NodeId>): Promise<HoistedDependencies | null> {
  const initialResult = getHoistedDependencies(opts)
  // Ahead of the graph, so that a workspace project always wins the alias over an
  // equally named transitive dependency instead of whichever symlink lands first.
  const hoistedWorkspaceDependencies = await hoistWorkspacePackages({
    ...opts,
    occupiedAliases: collectOccupiedAliases(initialResult, opts.directDepsByImporterId),
  })

  const reservedAliases = Object.values(hoistedWorkspaceDependencies)
    .flatMap(aliases => Object.keys(aliases))
  const result = reservedAliases.length === 0
    ? initialResult
    : getHoistedDependencies({ ...opts, reservedAliases })
  if (!result) {
    return Object.keys(hoistedWorkspaceDependencies).length === 0
      ? null
      : hoistedWorkspaceDependencies
  }
  const { hoistedDependencies, hoistedAliasesWithBins, hoistedDependenciesByNodeId } = result
  for (const [projectId, aliases] of Object.entries(hoistedWorkspaceDependencies)) {
    hoistedDependencies[projectId as ProjectId] = {
      ...hoistedDependencies[projectId as ProjectId],
      ...aliases,
    }
  }

  await symlinkHoistedDependencies(hoistedDependenciesByNodeId, {
    graph: opts.graph,
    privateHoistedModulesDir: opts.privateHoistedModulesDir,
    publicHoistedModulesDir: opts.publicHoistedModulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })

  // Here we only link the bins of the privately hoisted modules.
  // The bins of the publicly hoisted modules will be linked together with
  // the bins of the project's direct dependencies.
  // This is possible because the publicly hoisted modules
  // are in the same directory as the regular dependencies.
  await linkAllBins(opts.privateHoistedModulesDir, {
    extraNodePaths: opts.extraNodePath,
    hoistedAliasesWithBins,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })

  return hoistedDependencies
}

export interface GetHoistedDependenciesOpts<NodeId extends string> {
  graph: DependenciesGraph<NodeId>
  skipped: Set<DepPath>
  directDepsByImporterId: DirectDependenciesByImporterId<NodeId>
  importerIds?: ProjectId[]
  privateHoistPattern: string[]
  privateHoistedModulesDir: string
  publicHoistPattern: string[]
  publicHoistedModulesDir: string
  reservedAliases?: Iterable<string>
  hoistedWorkspacePackages?: Record<ProjectId, HoistedWorkspaceProject>
}

export function getHoistedDependencies<NodeId extends string> (opts: GetHoistedDependenciesOpts<NodeId>): HoistGraphResult<NodeId> | null {
  if (Object.keys(opts.graph ?? {}).length === 0) return null
  const rootDirectDeps = opts.directDepsByImporterId['.' as ProjectId] ?? new Map<string, NodeId>()
  const privateRootAliases = isSubdir(path.dirname(opts.publicHoistedModulesDir), opts.privateHoistedModulesDir)
    ? new Set<string>()
    : new Set(Array.from(rootDirectDeps.keys()).filter(createMatcher(opts.privateHoistPattern)))
  const { directDeps, step } = graphWalker(
    opts.graph,
    opts.directDepsByImporterId
  )
  const deps: Array<Dependency<NodeId>> = [
    {
      children: directDeps
        .reduce((acc, { alias, nodeId }) => {
          if (!Object.hasOwn(acc, alias)) {
            acc[alias] = nodeId
          }
          return acc
        }, privateRootAliases.size > 0 ? Object.fromEntries(rootDirectDeps) : {} as Record<string, NodeId>),
      nodeId: '' as NodeId,
      depth: -1,
    },
    ...getDependencies(0, step),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  return hoistGraph(deps, rootDirectDeps, {
    getAliasHoistType,
    privateRootAliases,
    graph: opts.graph,
    reservedAliases: opts.reservedAliases,
    skipped: opts.skipped,
  })
}

interface LinkAllBinsOptions {
  extraNodePaths?: string[]
  hoistedAliasesWithBins: string[]
  preferSymlinkedExecutables?: boolean
}

async function linkAllBins (modulesDir: string, opts: LinkAllBinsOptions): Promise<void> {
  const bin = path.join(modulesDir, '.bin')
  const warn: WarnFunction = (message, code) => {
    if (code === 'BINARIES_CONFLICT') return
    logger.info({ message, prefix: path.join(modulesDir, '../..') })
  }
  try {
    await linkBinsOfPkgsByAliases(opts.hoistedAliasesWithBins, bin, {
      allowExoticManifests: true,
      extraNodePaths: opts.extraNodePaths,
      modulesDir,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn,
    })
  } catch (err: any) { // eslint-disable-line
    // Some packages generate their commands with lifecycle hooks.
    // At this stage, such commands are not generated yet.
    // For now, we don't hoist such generated commands.
    // Related issue: https://github.com/pnpm/pnpm/issues/2071
  }
}

function getDependencies<NodeId extends string> (
  depth: number,
  step: GraphWalkerStep<NodeId>
): Array<Dependency<NodeId>> {
  const deps: Array<Dependency<NodeId>> = []
  const nextSteps: Array<GraphWalkerStep<NodeId>> = []
  for (const { node, nodeId, next } of step.dependencies) {
    deps.push({
      children: node.children,
      nodeId,
      depth,
    })

    nextSteps.push(next())
  }

  for (const depPath of step.missing) {
    // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
    // However, the skipped list currently contains package IDs, not dep paths.
    logger.debug({ message: `No entry for "${depPath}" in ${WANTED_LOCKFILE}` })
  }

  return [
    ...deps,
    ...(nextSteps.flatMap(getDependencies.bind(null, depth + 1)) as Array<Dependency<NodeId>>),
  ]
}

export interface Dependency<NodeId extends string> {
  children: Record<string, NodeId>
  nodeId: NodeId
  depth: number
}

interface HoistGraphOpts<NodeId extends string> {
  getAliasHoistType: GetAliasHoistType
  privateRootAliases: Set<string>
  graph: DependenciesGraph<NodeId>
  reservedAliases?: Iterable<string>
  skipped: Set<DepPath>
}

interface HoistGraphContext<NodeId extends string> extends HoistGraphOpts<NodeId> {
  currentSpecifiers: Map<string, NodeId>
  hoistedAliases: Set<string>
  hoistedAliasesWithBins: Set<string>
  hoistedDependencies: HoistedDependencies
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<NodeId>
}

interface HoistCandidate<NodeId extends string> {
  alias: string
  nodeId: NodeId
  node: DependenciesGraphNode<NodeId> | undefined
}

function hoistGraph<NodeId extends string> (
  depNodes: Array<Dependency<NodeId>>,
  currentSpecifiers: Map<string, NodeId>,
  opts: HoistGraphOpts<NodeId>
): HoistGraphResult<NodeId> {
  const ctx: HoistGraphContext<NodeId> = {
    ...opts,
    currentSpecifiers,
    hoistedAliases: new Set([
      ...Array.from(currentSpecifiers.keys()).filter(alias => !opts.privateRootAliases.has(alias)),
      ...opts.reservedAliases ?? [],
    ].map(alias => alias.toLowerCase())),
    hoistedAliasesWithBins: new Set<string>(),
    hoistedDependencies: Object.create(null),
    hoistedDependenciesByNodeId: new Map(),
  }

  for (const depNode of depNodes.sort(compareByDepthThenNodeId)) {
    for (const [childAlias, childNodeId] of Object.entries<NodeId>(depNode.children)) {
      hoistChild(ctx, {
        alias: childAlias,
        nodeId: childNodeId,
        node: opts.graph[childNodeId as NodeId],
      })
    }
  }

  return {
    hoistedDependencies: ctx.hoistedDependencies,
    hoistedDependenciesByNodeId: ctx.hoistedDependenciesByNodeId,
    hoistedAliasesWithBins: Array.from(ctx.hoistedAliasesWithBins),
  }
}

function compareByDepthThenNodeId<NodeId extends string> (left: Dependency<NodeId>, right: Dependency<NodeId>): number {
  const depthDiff = left.depth - right.depth
  return depthDiff === 0 ? lexCompare(left.nodeId, right.nodeId) : depthDiff
}

function hoistChild<NodeId extends string> (ctx: HoistGraphContext<NodeId>, child: HoistCandidate<NodeId>): void {
  const { alias: childAlias, nodeId: childNodeId, node } = child
  const hoist = getChildHoistType(ctx, child)
  if (!hoist) return
  const childAliasNormalized = childAlias.toLowerCase()
  if (ctx.hoistedAliases.has(childAliasNormalized)) return
  if (!ctx.hoistedDependenciesByNodeId.has(childNodeId)) {
    ctx.hoistedDependenciesByNodeId.set(childNodeId, {})
  }
  ctx.hoistedDependenciesByNodeId.get(childNodeId)![childAlias] = hoist
  if (node?.depPath == null || ctx.skipped.has(node.depPath)) return
  if (node.hasBin) {
    ctx.hoistedAliasesWithBins.add(childAlias)
  }
  ctx.hoistedAliases.add(childAliasNormalized)
  if (!ctx.hoistedDependencies[node.depPath]) {
    ctx.hoistedDependencies[node.depPath] = {}
  }
  ctx.hoistedDependencies[node.depPath][childAlias] = hoist
}

function getChildHoistType<NodeId extends string> (ctx: HoistGraphContext<NodeId>, child: HoistCandidate<NodeId>): HoistType | false {
  if (!ctx.privateRootAliases.has(child.alias)) return ctx.getAliasHoistType(child.alias)
  // Only the root's own version of a root dependency may take its alias, and a skipped one keeps it reserved.
  if (ctx.currentSpecifiers.get(child.alias) !== child.nodeId) return false
  if (child.node?.depPath != null && ctx.skipped.has(child.node.depPath)) return false
  return 'private'
}

interface SymlinkHoistedDependenciesOpts<NodeId extends string> {
  graph: DependenciesGraph<NodeId>
  privateHoistedModulesDir: string
  publicHoistedModulesDir: string
  virtualStoreDir: string
}

async function symlinkHoistedDependencies<NodeId extends string> (
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<NodeId>,
  opts: SymlinkHoistedDependenciesOpts<NodeId>
): Promise<void> {
  const symlink = symlinkHoistedDependency.bind(null, {
    virtualStoreDir: opts.virtualStoreDir,
    installStateDir: path.dirname(opts.privateHoistedModulesDir),
  })
  await Promise.all(Array.from(hoistedDependenciesByNodeId.entries(), async ([hoistedDepNodeId, pkgAliases]) => {
    const node = opts.graph[hoistedDepNodeId]
    if (node == null) {
      // This dependency is probably a skipped optional dependency.
      hoistLogger.debug({ hoistFailedFor: hoistedDepNodeId })
      return
    }
    const depLocation = node.dir
    await Promise.all(Object.entries(pkgAliases).map(async ([pkgAlias, hoistType]) => {
      const dest = path.join(selectHoistedModulesDir(hoistType, opts), pkgAlias)
      return symlink(depLocation, dest)
    }))
  }))
}
