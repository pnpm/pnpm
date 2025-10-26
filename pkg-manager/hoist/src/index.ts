import fs from 'fs'
import path from 'path'
import { linkLogger } from '@pnpm/core-loggers'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { linkBinsOfPkgsByAliases, type WarnFunction } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import { createMatcher } from '@pnpm/matcher'
import { type DepPath, type HoistedDependencies, type ProjectId, type DependenciesField } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import isSubdir from 'is-subdir'
import resolveLinkTarget from 'resolve-link-target'
import symlinkDir from 'symlink-dir'

export interface DependenciesGraphNode<T extends string> {
  dir: string
  children: Record<string, T>
  optionalDependencies: Set<string>
  hasBin: boolean
  name: string
  depPath: DepPath
}

export type DependenciesGraph<T extends string> = Record<T, DependenciesGraphNode<T>>

export interface DirectDependenciesByImporterId<T extends string> {
  [importerId: string]: Map<string, T>
}

const hoistLogger = logger('hoist')

export interface HoistOpts<T extends string> extends GetHoistedDependenciesOpts<T> {
  extraNodePath?: string[]
  preferSymlinkedExecutables?: boolean
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
}

export async function hoist<T extends string> (opts: HoistOpts<T>): Promise<HoistedDependencies | null> {
  const result = getHoistedDependencies(opts)
  if (!result) return null
  const { hoistedDependencies, hoistedAliasesWithBins, hoistedDependenciesByNodeId } = result

  await symlinkHoistedDependencies(hoistedDependenciesByNodeId, {
    graph: opts.graph,
    directDepsByImporterId: opts.directDepsByImporterId,
    privateHoistedModulesDir: opts.privateHoistedModulesDir,
    publicHoistedModulesDir: opts.publicHoistedModulesDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    hoistedWorkspacePackages: opts.hoistedWorkspacePackages,
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

export interface GetHoistedDependenciesOpts<T extends string> {
  graph: DependenciesGraph<T>
  skipped: Set<DepPath>
  directDepsByImporterId: DirectDependenciesByImporterId<T>
  importerIds?: ProjectId[]
  privateHoistPattern: string[]
  privateHoistedModulesDir: string
  publicHoistPattern: string[]
  publicHoistedModulesDir: string
  hoistedWorkspacePackages?: Record<ProjectId, HoistedWorkspaceProject>
}

export interface HoistedWorkspaceProject {
  name: string
  dir: string
}

export function getHoistedDependencies<T extends string> (opts: GetHoistedDependenciesOpts<T>): HoistGraphResult<T> | null {
  if (Object.keys(opts.graph ?? {}).length === 0) return null
  const { directDeps, step } = graphWalker(
    opts.graph,
    opts.directDepsByImporterId
  )
  // We want to hoist all the workspace packages, not only those that are in the dependencies
  // of any other workspace packages.
  // That is why we can't just simply use the lockfile walker to include links to local workspace packages too.
  // We have to explicitly include all the workspace packages.
  const hoistedWorkspaceDeps: Record<string, ProjectId> = Object.fromEntries(
    Object.entries(opts.hoistedWorkspacePackages ?? {})
      .map(([id, { name }]) => [name, id as ProjectId])
  )
  const deps: Array<Dependency<T>> = [
    {
      children: {
        ...hoistedWorkspaceDeps,
        ...directDeps
          .reduce((acc, { alias, nodeId }) => {
            if (!acc[alias]) {
              acc[alias] = nodeId
            }
            return acc
          }, {} as Record<string, T>),
      },
      nodeId: '' as T,
      depth: -1,
    },
    ...getDependencies(0, step),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  return hoistGraph(deps, opts.directDepsByImporterId['.' as ProjectId] ?? new Map(), {
    getAliasHoistType,
    graph: opts.graph,
    skipped: opts.skipped,
  })
}

type GetAliasHoistType = (alias: string) => 'private' | 'public' | false

function createGetAliasHoistType (
  publicHoistPattern: string[],
  privateHoistPattern: string[]
): GetAliasHoistType {
  const publicMatcher = createMatcher(publicHoistPattern)
  const privateMatcher = createMatcher(privateHoistPattern)
  return (alias: string) => {
    if (publicMatcher(alias)) return 'public'
    if (privateMatcher(alias)) return 'private'
    return false
  }
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

function getDependencies<T extends string> (
  depth: number,
  step: GraphWalkerStep<T>
): Array<Dependency<T>> {
  const deps: Array<Dependency<T>> = []
  const nextSteps: Array<GraphWalkerStep<T>> = []
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
    ...(nextSteps.flatMap(getDependencies.bind(null, depth + 1)) as Array<Dependency<T>>),
  ]
}

export interface Dependency<T extends string> {
  children: Record<string, T | ProjectId>
  nodeId: T
  depth: number
}

interface HoistGraphResult<T extends string> {
  hoistedDependencies: HoistedDependencies
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<T>
  hoistedAliasesWithBins: string[]
}

type HoistedDependenciesByNodeId<T extends string> = Map<T | ProjectId, Record<string, 'public' | 'private'>>

function hoistGraph<T extends string> (
  depNodes: Array<Dependency<T>>,
  currentSpecifiers: Map<string, T>,
  opts: {
    getAliasHoistType: GetAliasHoistType
    graph: DependenciesGraph<T>
    skipped: Set<DepPath>
  }
): HoistGraphResult<T> {
  const hoistedAliases = new Set(currentSpecifiers.keys())
  const hoistedDependencies: HoistedDependencies = Object.create(null)
  const hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<T> = new Map()
  const hoistedAliasesWithBins = new Set<string>()

  depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? lexCompare(a.nodeId, b.nodeId) : depthDiff
    })
    // build the alias map and the id map
    .forEach((depNode) => {
      for (const [childAlias, childNodeId] of Object.entries<T | ProjectId>(depNode.children)) {
        const hoist = opts.getAliasHoistType(childAlias)
        if (!hoist) continue
        const childAliasNormalized = childAlias.toLowerCase()
        // if this alias has already been taken, skip it
        if (hoistedAliases.has(childAliasNormalized)) {
          continue
        }
        if (!hoistedDependenciesByNodeId.has(childNodeId)) {
          hoistedDependenciesByNodeId.set(childNodeId, {})
        }
        hoistedDependenciesByNodeId.get(childNodeId)![childAlias] = hoist
        const node = opts.graph[childNodeId as T]
        if (node?.depPath == null || opts.skipped.has(node.depPath)) {
          continue
        }
        if (node.hasBin) {
          hoistedAliasesWithBins.add(childAlias)
        }
        hoistedAliases.add(childAliasNormalized)
        if (!hoistedDependencies[node.depPath]) {
          hoistedDependencies[node.depPath] = {}
        }
        hoistedDependencies[node.depPath][childAlias] = hoist
      }
    })

  return {
    hoistedDependencies,
    hoistedDependenciesByNodeId,
    hoistedAliasesWithBins: Array.from(hoistedAliasesWithBins),
  }
}

async function symlinkHoistedDependencies<T extends string> (
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<T>,
  opts: {
    graph: DependenciesGraph<T>
    directDepsByImporterId: DirectDependenciesByImporterId<T>
    privateHoistedModulesDir: string
    publicHoistedModulesDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    hoistedWorkspacePackages?: Record<string, HoistedWorkspaceProject>
  }
): Promise<void> {
  const symlink = symlinkHoistedDependency.bind(null, opts)
  const promises: Array<Promise<void>> = []
  for (const [hoistedDepNodeId, pkgAliases] of hoistedDependenciesByNodeId.entries()) {
    promises.push((async () => {
      const node = opts.graph[hoistedDepNodeId as T]
      let depLocation!: string
      if (node) {
        depLocation = node.dir
      } else {
        if (!opts.directDepsByImporterId[hoistedDepNodeId as ProjectId]) {
          // This dependency is probably a skipped optional dependency.
          hoistLogger.debug({ hoistFailedFor: hoistedDepNodeId })
          return
        }
        depLocation = opts.hoistedWorkspacePackages![hoistedDepNodeId].dir
      }
      await Promise.all(Object.entries(pkgAliases).map(async ([pkgAlias, hoistType]) => {
        const targetDir = hoistType === 'public'
          ? opts.publicHoistedModulesDir
          : opts.privateHoistedModulesDir
        const dest = path.join(targetDir, pkgAlias)
        return symlink(depLocation, dest)
      }))
    })())
  }
  await Promise.all(promises)
}

async function symlinkHoistedDependency (
  opts: { virtualStoreDir: string },
  depLocation: string,
  dest: string
): Promise<void> {
  try {
    await symlinkDir(depLocation, dest, { overwrite: false })
    linkLogger.debug({ target: dest, link: depLocation })
    return
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'EEXIST' && err.code !== 'EISDIR') throw err
  }
  let existingSymlink!: string
  try {
    existingSymlink = await resolveLinkTarget(dest)
  } catch {
    hoistLogger.debug({
      skipped: dest,
      reason: 'a directory is present at the target location',
    })
    return
  }
  if (!isSubdir(opts.virtualStoreDir, existingSymlink)) {
    hoistLogger.debug({
      skipped: dest,
      existingSymlink,
      reason: 'an external symlink is present at the target location',
    })
    return
  }
  await fs.promises.unlink(dest)
  await symlinkDir(depLocation, dest)
  linkLogger.debug({ target: dest, link: depLocation })
}

export function graphWalker<T extends string> (
  graph: DependenciesGraph<T>,
  directDepsByImporterId: DirectDependenciesByImporterId<T>,
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<DepPath>
  }
): GraphWalker<T> {
  const startNodeIds = [] as T[]
  const allDirectDeps = [] as Array<{ alias: string, nodeId: T }>

  for (const directDeps of Object.values(directDepsByImporterId)) {
    for (const [alias, nodeId] of directDeps.entries()) {
      const depNode = graph[nodeId]
      if (depNode == null) continue
      startNodeIds.push(nodeId)
      allDirectDeps.push({ alias, nodeId })
    }
  }
  const visited = new Set<T>()
  return {
    directDeps: allDirectDeps,
    step: makeStep({
      includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
      graph,
      visited,
      skipped: opts?.skipped,
    }, startNodeIds),
  }
}

function makeStep<T extends string> (
  ctx: {
    includeOptionalDependencies: boolean
    graph: DependenciesGraph<T>
    visited: Set<T>
    skipped?: Set<DepPath>
  },
  nextNodeIds: T[]
): GraphWalkerStep<T> {
  const result: GraphWalkerStep<T> = {
    dependencies: [],
    links: [],
    missing: [],
  }
  const _next = collectChildNodeIds.bind(null, {
    includeOptionalDependencies: ctx.includeOptionalDependencies,
  })
  for (const nodeId of nextNodeIds) {
    if (ctx.visited.has(nodeId)) continue
    ctx.visited.add(nodeId)
    const node = ctx.graph[nodeId]
    if (node == null) {
      if (nodeId.startsWith('link:')) {
        result.links.push(nodeId)
        continue
      }
      result.missing.push(nodeId)
      continue
    }
    if (ctx.skipped?.has(node.depPath)) continue
    result.dependencies.push({
      nodeId,
      next: () => makeStep<T>(ctx, _next(node) as T[]),
      node,
    })
  }
  return result
}

function collectChildNodeIds<T extends string> (opts: { includeOptionalDependencies: boolean }, nextPkg: DependenciesGraphNode<T>): T[] {
  if (opts.includeOptionalDependencies) {
    return Object.values(nextPkg.children)
  } else {
    const nextNodeIds: T[] = []
    for (const [alias, nodeId] of Object.entries(nextPkg.children)) {
      if (!nextPkg.optionalDependencies.has(alias)) {
        nextNodeIds.push(nodeId)
      }
    }
    return nextNodeIds
  }
}

export interface GraphWalker<T extends string> {
  directDeps: Array<{
    alias: string
    nodeId: T
  }>
  step: GraphWalkerStep<T>
}

export interface GraphWalkerStep<T extends string> {
  dependencies: Array<GraphDependency<T>>
  links: string[]
  missing: string[]
}

export interface GraphDependency<T extends string> {
  nodeId: T
  node: DependenciesGraphNode<T>
  next: () => GraphWalkerStep<T>
}
