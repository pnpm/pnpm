import fs from 'fs'
import path from 'path'
import { linkLogger } from '@pnpm/core-loggers'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { linkBinsOfPkgsByAliases, type WarnFunction } from '@pnpm/link-bins'
import {
  type LockfileObject,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { createMatcher } from '@pnpm/matcher'
import { type DepPath, type HoistedDependencies, type ProjectId, type DependenciesField } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import * as dp from '@pnpm/dependency-path'
import isSubdir from 'is-subdir'
import mapObjIndexed from 'ramda/src/mapObjIndexed'
import resolveLinkTarget from 'resolve-link-target'
import symlinkDir from 'symlink-dir'

export interface DependenciesGraphNode {
  dir: string
  children: Record<string, string>
  optionalDependencies: Set<string>
  hasBin: boolean
  name: string
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export interface DirectDependenciesByImporterId {
  [importerId: string]: { [alias: string]: string }
}

const hoistLogger = logger('hoist')

export interface HoistOpts extends GetHoistedDependenciesOpts {
  extraNodePath?: string[]
  preferSymlinkedExecutables?: boolean
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
}

export async function hoist (opts: HoistOpts): Promise<HoistedDependencies> {
  const result = getHoistedDependencies(opts)
  if (!result) return {}
  const { hoistedDependencies, hoistedAliasesWithBins } = result

  await symlinkHoistedDependencies(hoistedDependencies, {
    graph: opts.graph,
    directDepsByImporterIds: opts.directDepsByImporterIds,
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

export interface GetHoistedDependenciesOpts {
  graph: DependenciesGraph,
  directDepsByImporterIds: DirectDependenciesByImporterId,
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

export function getHoistedDependencies (opts: GetHoistedDependenciesOpts): HoistGraphResult | null {
  const { directDeps, step } = graphWalker(
    opts.graph,
    opts.directDepsByImporterIds
  )
  // We want to hoist all the workspace packages, not only those that are in the dependencies
  // of any other workspace packages.
  // That is why we can't just simply use the lockfile walker to include links to local workspace packages too.
  // We have to explicitly include all the workspace packages.
  const hoistedWorkspaceDeps: Record<string, ProjectId> = Object.fromEntries(
    Object.entries(opts.hoistedWorkspacePackages ?? {})
      .map(([id, { name }]) => [name, id as ProjectId])
  )
  const deps: Dependency[] = [
    {
      children: {
        ...hoistedWorkspaceDeps,
        ...directDeps
          .reduce((acc, { alias, nodeId }) => {
            if (!acc[alias]) {
              acc[alias] = nodeId
            }
            return acc
          }, {} as Record<string, string>),
      },
      nodeId: '',
      depth: -1,
    },
    ...getDependencies(0, step),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  return hoistGraph(deps, opts.directDepsByImporterIds['.' as ProjectId] ?? {}, {
    getAliasHoistType,
    graph: opts.graph,
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

function getDependencies (
  depth: number,
  step: LockfileWalkerStep
): Dependency[] {
  const deps: Dependency[] = []
  const nextSteps: LockfileWalkerStep[] = []
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
    ...nextSteps.flatMap(getDependencies.bind(null, depth + 1)),
  ]
}

export interface Dependency {
  children: Record<string, string | ProjectId>
  nodeId: string
  depth: number
}

interface HoistGraphResult {
  hoistedDependencies: HoistedDependencies
  hoistedAliasesWithBins: string[]
}

function hoistGraph (
  depNodes: Dependency[],
  currentSpecifiers: Record<string, string>,
  opts: {
    getAliasHoistType: GetAliasHoistType
    graph: DependenciesGraph
  }
): HoistGraphResult {
  const hoistedAliases = new Set(Object.keys(currentSpecifiers))
  const hoistedDependencies: HoistedDependencies = {}
  const hoistedAliasesWithBins = new Set<string>()

  depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? lexCompare(a.nodeId, b.nodeId) : depthDiff
    })
    // build the alias map and the id map
    .forEach((depNode) => {
      for (const [childAlias, childPath] of Object.entries<string | ProjectId>(depNode.children)) {
        const hoist = opts.getAliasHoistType(childAlias)
        if (!hoist) continue
        const childAliasNormalized = childAlias.toLowerCase()
        // if this alias has already been taken, skip it
        if (hoistedAliases.has(childAliasNormalized)) {
          continue
        }
        if (opts.graph?.[childPath as DepPath]?.hasBin) {
          hoistedAliasesWithBins.add(childAlias)
        }
        hoistedAliases.add(childAliasNormalized)
        if (!hoistedDependencies[childPath]) {
          hoistedDependencies[childPath] = {}
        }
        hoistedDependencies[childPath][childAlias] = hoist
      }
    })

  return { hoistedDependencies, hoistedAliasesWithBins: Array.from(hoistedAliasesWithBins) }
}

async function symlinkHoistedDependencies (
  hoistedDependencies: HoistedDependencies,
  opts: {
    graph: DependenciesGraph
    directDepsByImporterIds: DirectDependenciesByImporterId,
    privateHoistedModulesDir: string
    publicHoistedModulesDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    hoistedWorkspacePackages?: Record<string, HoistedWorkspaceProject>
  }
): Promise<void> {
  const symlink = symlinkHoistedDependency.bind(null, opts)
  await Promise.all(
    Object.entries(hoistedDependencies)
      .map(async ([hoistedDepId, pkgAliases]) => {
        const node = opts.graph[hoistedDepId]
        let depLocation!: string
        if (node) {
          depLocation = hoistedDepId
        } else {
          if (!opts.directDepsByImporterIds[hoistedDepId as ProjectId]) {
            // This dependency is probably a skipped optional dependency.
            hoistLogger.debug({ hoistFailedFor: hoistedDepId })
            return
          }
          depLocation = opts.hoistedWorkspacePackages![hoistedDepId].dir
        }
        await Promise.all(Object.entries(pkgAliases).map(async ([pkgAlias, hoistType]) => {
          const targetDir = hoistType === 'public'
            ? opts.publicHoistedModulesDir
            : opts.privateHoistedModulesDir
          const dest = path.join(targetDir, pkgAlias)
          return symlink(depLocation, dest)
        }))
      })
  )
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

export function graphWalker (
  graph: DependenciesGraph,
  directDepsByImporterIds: DirectDependenciesByImporterId,
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<DepPath>
  }
): LockfileWalker {
  const walked = new Set<DepPath>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])
  const entryNodes = [] as string[]
  const allDirectDeps = [] as Array<{ alias: string, nodeId: string}>

  for (const [importerId, directDeps] of Object.entries(directDepsByImporterIds)) {
    Object.entries(directDeps)
      .forEach(([alias, nodeId]) => {
        const depNode = graph[nodeId]
        if (depNode == null) return
        entryNodes.push(nodeId)
        allDirectDeps.push({ alias, nodeId })
      })
  }
  return {
    directDeps: allDirectDeps,
    step: step({
      includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
      graph,
      walked,
    }, entryNodes),
  }
}

function step (
  ctx: {
    includeOptionalDependencies: boolean
    graph: DependenciesGraph,
    walked: Set<string>
  },
  nextNodeIds: string[]
): LockfileWalkerStep {
  const result: LockfileWalkerStep = {
    dependencies: [],
    links: [],
    missing: [],
  }
  for (const nodeId of nextNodeIds) {
    if (ctx.walked.has(nodeId)) continue
    ctx.walked.add(nodeId)
    const node = ctx.graph[nodeId]
    if (node == null) {
      if (nodeId.startsWith('link:')) {
        result.links.push(nodeId)
        continue
      }
      result.missing.push(nodeId)
      continue
    }
    result.dependencies.push({
      nodeId,
      next: () => step(ctx, next({ includeOptionalDependencies: ctx.includeOptionalDependencies }, node)),
      node,
    })
  }
  return result
}

function next (opts: { includeOptionalDependencies: boolean }, nextPkg: DependenciesGraphNode): DepPath[] {
  return Object.values(nextPkg.children)
    .filter((nodeId) => nodeId !== null) as DepPath[]
}

export interface LockfileWalker {
  directDeps: Array<{
    alias: string
    nodeId: string
  }>
  step: LockfileWalkerStep
}

export interface LockfileWalkerStep {
  dependencies: LockedDependency[]
  links: string[]
  missing: string[]
}

export interface LockedDependency {
  nodeId: string
  node: DependenciesGraphNode
  next: () => LockfileWalkerStep
}
