import fs from 'node:fs'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import util from 'node:util'

import { linkBinsOfPkgsByAliases, type WarnFunction } from '@pnpm/bins.linker'
import { createMatcher } from '@pnpm/config.matcher'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { linkLogger } from '@pnpm/core-loggers'
import { withFileLockRetryAsync } from '@pnpm/fs.graceful-fs'
import { findCommonPathAncestor, prepareWorkspaceModulesDir, validateWorkspaceModulesDir } from '@pnpm/fs.symlink-dependency'
import { logger } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { DependenciesField, DepPath, HoistedDependencies, ProjectId } from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { resolveLinkTarget } from 'resolve-link-target'
import { symlinkDir } from 'symlink-dir'

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
  beforeWorkspaceLinks?: (hoistedDependencies: HoistedDependencies) => Promise<(() => Promise<void>) | void>
  extraNodePath?: string[]
  preferSymlinkedExecutables?: boolean
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
}

export async function hoist<T extends string> (opts: HoistOpts<T>): Promise<HoistedDependencies | null> {
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
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
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
  reservedAliases?: Iterable<string>
  hoistedWorkspacePackages?: Record<ProjectId, HoistedWorkspaceProject>
}

export interface HoistedWorkspaceProject {
  name: string
  dir: string
}

export interface HoistWorkspacePackagesOpts<T extends string> {
  beforeWorkspaceLinks?: (hoistedDependencies: HoistedDependencies) => Promise<(() => Promise<void>) | void>
  directDepsByImporterId: DirectDependenciesByImporterId<T>
  graph: DependenciesGraph<T>
  hoistedWorkspacePackages?: Record<ProjectId, HoistedWorkspaceProject>
  occupiedAliases?: OccupiedAliases
  privateHoistedModulesDir: string
  privateHoistPattern: string[]
  publicHoistedModulesDir: string
  publicHoistPattern: string[]
  virtualStoreDir: string
}

/**
 * Symlinks the workspace projects that the hoist patterns select.
 *
 * Every named project of the workspace is a candidate, not only the ones some other
 * project depends on, so this pass reads the projects rather than the dependency
 * graph. That also makes it independent of the graph: it is the whole of the work
 * for a workspace that installs nothing from a registry, and an install that changes
 * no dependency can run it on its own, without walking the graph again.
 *
 * A project loses its alias to a direct dependency of any project, which is hoisted
 * from the graph instead. A dependency with no node in the graph, a `workspace:`
 * link among them, claims nothing: the graph walk skips it too.
 */
export async function hoistWorkspacePackages<T extends string> (opts: HoistWorkspacePackagesOpts<T>): Promise<HoistedDependencies> {
  if (opts.hoistedWorkspacePackages == null) {
    await opts.beforeWorkspaceLinks?.({})
    return {}
  }
  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)
  const aliasesTakenByDependencies = new Set<string>()
  for (const directDeps of Object.values(opts.directDepsByImporterId)) {
    for (const [alias, nodeId] of directDeps.entries()) {
      if (opts.graph[nodeId] == null) continue
      aliasesTakenByDependencies.add(alias.toLowerCase())
    }
  }
  const symlink = symlinkHoistedDependency.bind(null, {
    virtualStoreDir: opts.virtualStoreDir,
    internalPnpmDir: path.dirname(opts.privateHoistedModulesDir),
  })
  let placementCandidates: Array<[ProjectId, HoistedWorkspaceProject, 'private' | 'public']> = []
  for (const [projectId, project] of Object.entries(opts.hoistedWorkspacePackages) as Array<[ProjectId, HoistedWorkspaceProject]>) {
    const { name } = project
    const hoistType = getAliasHoistType(name)
    if (!hoistType || aliasesTakenByDependencies.has(name.toLowerCase())) continue
    aliasesTakenByDependencies.add(name.toLowerCase())
    placementCandidates.push([projectId, project, hoistType])
  }
  const conflictingProjectIds = findConflictingWorkspaceProjectIds(placementCandidates, opts.occupiedAliases)
  placementCandidates = placementCandidates.filter(([projectId]) => !conflictingProjectIds.has(projectId))
  const hoistedDependencies = Object.fromEntries(placementCandidates.map(([projectId, { name }, hoistType]) => [
    projectId,
    { [name]: hoistType },
  ])) as HoistedDependencies
  const rollback = await opts.beforeWorkspaceLinks?.(hoistedDependencies)
  let placements: Array<readonly [ProjectId, HoistedWorkspaceProject, 'private' | 'public', string]>
  try {
    placements = await Promise.all(placementCandidates.map(async ([projectId, project, hoistType]) => {
      const targetDir = hoistType === 'public'
        ? opts.publicHoistedModulesDir
        : opts.privateHoistedModulesDir
      const trustedRoot = findCommonPathAncestor(opts.publicHoistedModulesDir, targetDir) ?? path.parse(path.resolve(targetDir)).root
      const destination = await prepareWorkspaceModulesDir(targetDir, project.name, trustedRoot)
      return [projectId, project, hoistType, destination] as const
    }))
  } catch (error: unknown) {
    await runWorkspaceRollback(rollback, opts.privateHoistedModulesDir)
    throw error
  }
  const results = await Promise.allSettled(placements.map(async ([, { dir }, , destination]) => symlink(dir, destination)))
  const failure = results.find((result): result is PromiseRejectedResult => result.status === 'rejected')
  if (failure != null) {
    const cleanupResults = await Promise.allSettled(placements.map(async ([, { dir }, , destination], index) => {
      if (results[index].status !== 'fulfilled') return
      await removeWorkspaceLinkIfTarget(destination, dir)
    }))
    reportWorkspaceRollbackFailures(cleanupResults, opts.privateHoistedModulesDir)
    await runWorkspaceRollback(rollback, opts.privateHoistedModulesDir)
    throw failure.reason
  }
  return hoistedDependencies
}

export async function pruneStaleWorkspaceHoists (
  previous: HoistedDependencies,
  next: HoistedDependencies,
  projectIds: Set<ProjectId>,
  privateHoistedModulesDir: string,
  publicHoistedModulesDir: string
): Promise<() => Promise<void>> {
  const staleLinks = (await Promise.all(Array.from(projectIds).flatMap((projectId) => {
    const nextAliases = next[projectId]
    return Object.entries(previous[projectId] ?? {}).flatMap(([alias, hoistType]) => {
      if (nextAliases?.[alias] === hoistType) return []
      const modulesDir = hoistType === 'public'
        ? publicHoistedModulesDir
        : privateHoistedModulesDir
      const trustedRoot = findCommonPathAncestor(publicHoistedModulesDir, modulesDir) ?? path.parse(path.resolve(modulesDir)).root
      return [(async () => {
        const destination = await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
        let target: string
        try {
          const rawTarget = await fs.promises.readlink(destination)
          target = path.resolve(path.dirname(destination), rawTarget)
        } catch (error: unknown) {
          if (util.types.isNativeError(error) && 'code' in error && (error.code === 'ENOENT' || error.code === 'EINVAL')) {
            return undefined
          }
          throw error
        }
        return { alias, destination, modulesDir, target, trustedRoot }
      })()]
    })
  }))).filter(link => link != null)
  const removalResults = await Promise.allSettled(staleLinks.map(async ({ alias, destination, modulesDir, trustedRoot }) => {
    await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
    await fs.promises.unlink(destination)
  }))
  const failure = removalResults.find((result): result is PromiseRejectedResult => result.status === 'rejected')
  const restore = async (links: typeof staleLinks): Promise<void> => {
    await Promise.all(links.map(async ({ alias, destination, modulesDir, target, trustedRoot }) => {
      await prepareWorkspaceModulesDir(modulesDir, alias, trustedRoot)
      await fs.promises.symlink(target, destination, process.platform === 'win32' ? 'junction' : 'dir')
    }))
  }
  if (failure != null) {
    await restore(staleLinks.filter((_, index) => removalResults[index].status === 'fulfilled'))
    throw failure.reason
  }
  const parentCleanupResults = await Promise.allSettled(staleLinks.map(async ({ alias, destination, modulesDir, trustedRoot }) => {
    await removeEmptyWorkspaceParents(modulesDir, alias, path.dirname(destination), trustedRoot)
  }))
  const parentCleanupFailure = parentCleanupResults.find((result): result is PromiseRejectedResult => result.status === 'rejected')
  if (parentCleanupFailure != null) {
    await restore(staleLinks)
    throw parentCleanupFailure.reason
  }
  return async () => restore(staleLinks)
}

async function removeEmptyWorkspaceParents (modulesDir: string, alias: string, parent: string, trustedRoot: string): Promise<void> {
  if (parent === modulesDir) return
  await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
  try {
    await fs.promises.rmdir(parent)
  } catch (error: unknown) {
    if (util.types.isNativeError(error) && 'code' in error && ['ENOENT', 'ENOTEMPTY', 'EEXIST'].includes(String(error.code))) return
    throw error
  }
  await removeEmptyWorkspaceParents(modulesDir, alias, path.dirname(parent), trustedRoot)
}

async function runWorkspaceRollback (rollback: (() => Promise<void>) | void, prefix: string): Promise<void> {
  if (rollback == null) return
  try {
    await rollback()
  } catch (error: unknown) {
    hoistLogger.warn({ message: `Failed to roll back workspace hoist links: ${String(error)}`, prefix })
  }
}

function reportWorkspaceRollbackFailures (results: PromiseSettledResult<void>[], prefix: string): void {
  for (const result of results) {
    if (result.status === 'rejected') {
      hoistLogger.warn({ message: `Failed to clean up a workspace hoist link: ${String(result.reason)}`, prefix })
    }
  }
}

function findConflictingWorkspaceProjectIds (
  candidates: Array<[ProjectId, HoistedWorkspaceProject, 'private' | 'public']>,
  occupiedAliases?: OccupiedAliases
): Set<ProjectId> {
  const conflictingProjectIds = new Set<ProjectId>()
  for (const hoistType of ['private', 'public'] as const) {
    const projectIdsByName = new Map<string, ProjectId[]>()
    for (const [projectId, project, candidateHoistType] of candidates) {
      if (candidateHoistType !== hoistType) continue
      const name = project.name.toLowerCase()
      if (occupiedAliases != null && conflictsWithOccupiedAlias(name, occupiedAliases[hoistType])) {
        conflictingProjectIds.add(projectId)
        continue
      }
      const projectIds = projectIdsByName.get(name)
      if (projectIds == null) {
        projectIdsByName.set(name, [projectId])
      } else {
        projectIds.push(projectId)
      }
    }
    const activeAncestors: Array<[string, ProjectId[]]> = []
    for (const current of [...projectIdsByName.entries()].sort(([left], [right]) => compareWorkspaceAliases(left, right))) {
      while (activeAncestors.length > 0 && !current[0].startsWith(`${activeAncestors.at(-1)![0]}/`)) {
        activeAncestors.pop()
      }
      if (activeAncestors.length > 0) {
        for (const [, projectIds] of activeAncestors) {
          for (const projectId of projectIds) conflictingProjectIds.add(projectId)
        }
        for (const projectId of current[1]) conflictingProjectIds.add(projectId)
      }
      activeAncestors.push(current)
    }
  }
  return conflictingProjectIds
}

interface AliasIndex {
  aliases: Set<string>
  ancestors: Set<string>
}

interface OccupiedAliases {
  private: AliasIndex
  public: AliasIndex
}

function collectOccupiedAliases<T extends string> (
  result: HoistGraphResult<T> | null,
  directDepsByImporterId: DirectDependenciesByImporterId<T>
): OccupiedAliases {
  const occupiedAliases: OccupiedAliases = {
    private: { aliases: new Set(), ancestors: new Set() },
    public: { aliases: new Set(), ancestors: new Set() },
  }
  if (result != null) {
    for (const aliases of result.hoistedDependenciesByNodeId.values()) {
      for (const [alias, hoistType] of Object.entries(aliases)) {
        addOccupiedAlias(alias, occupiedAliases[hoistType])
      }
    }
  }
  const rootDirectDeps = directDepsByImporterId['.' as ProjectId]
  if (rootDirectDeps != null) {
    for (const alias of rootDirectDeps.keys()) addOccupiedAlias(alias, occupiedAliases.public)
  }
  return occupiedAliases
}

function addOccupiedAlias (alias: string, index: AliasIndex): void {
  const components = alias.toLowerCase().split('/')
  index.aliases.add(components.join('/'))
  for (let length = 1; length < components.length; length++) {
    index.ancestors.add(components.slice(0, length).join('/'))
  }
}

function conflictsWithOccupiedAlias (alias: string, index: AliasIndex): boolean {
  if (index.ancestors.has(alias)) return true
  const components = alias.split('/')
  for (let length = 1; length < components.length; length++) {
    if (index.aliases.has(components.slice(0, length).join('/'))) return true
  }
  return false
}

function compareWorkspaceAliases (left: string, right: string): number {
  const leftComponents = left.split('/')
  const rightComponents = right.split('/')
  const length = Math.min(leftComponents.length, rightComponents.length)
  for (let index = 0; index < length; index++) {
    const compared = lexCompare(leftComponents[index], rightComponents[index])
    if (compared !== 0) return compared
  }
  return leftComponents.length - rightComponents.length
}

async function removeWorkspaceLinkIfTarget (destination: string, target: string): Promise<void> {
  let resolvedTarget: string
  try {
    resolvedTarget = await resolveLinkTarget(destination)
  } catch (error: unknown) {
    if (util.types.isNativeError(error) && 'code' in error && (error.code === 'ENOENT' || error.code === 'EINVAL')) return
    throw error
  }
  if (resolvedTarget === target) await fs.promises.unlink(destination)
}

export function getHoistedDependencies<T extends string> (opts: GetHoistedDependenciesOpts<T>): HoistGraphResult<T> | null {
  if (Object.keys(opts.graph ?? {}).length === 0) return null
  const { directDeps, step } = graphWalker(
    opts.graph,
    opts.directDepsByImporterId
  )
  const deps: Array<Dependency<T>> = [
    {
      children: directDeps
        .reduce((acc, { alias, nodeId }) => {
          if (!acc[alias]) {
            acc[alias] = nodeId
          }
          return acc
        }, {} as Record<string, T>),
      nodeId: '' as T,
      depth: -1,
    },
    ...getDependencies(0, step),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  return hoistGraph(deps, opts.directDepsByImporterId['.' as ProjectId] ?? new Map(), {
    getAliasHoistType,
    graph: opts.graph,
    reservedAliases: opts.reservedAliases,
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
  children: Record<string, T>
  nodeId: T
  depth: number
}

interface HoistGraphResult<T extends string> {
  hoistedDependencies: HoistedDependencies
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<T>
  hoistedAliasesWithBins: string[]
}

type HoistedDependenciesByNodeId<T extends string> = Map<T, Record<string, 'public' | 'private'>>

function hoistGraph<T extends string> (
  depNodes: Array<Dependency<T>>,
  currentSpecifiers: Map<string, T>,
  opts: {
    getAliasHoistType: GetAliasHoistType
    graph: DependenciesGraph<T>
    reservedAliases?: Iterable<string>
    skipped: Set<DepPath>
  }
): HoistGraphResult<T> {
  const hoistedAliases = new Set([
    ...currentSpecifiers.keys(),
    ...opts.reservedAliases ?? [],
  ].map(alias => alias.toLowerCase()))
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
      for (const [childAlias, childNodeId] of Object.entries<T>(depNode.children)) {
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
    privateHoistedModulesDir: string
    publicHoistedModulesDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
  }
): Promise<void> {
  const symlink = symlinkHoistedDependency.bind(null, {
    virtualStoreDir: opts.virtualStoreDir,
    internalPnpmDir: path.dirname(opts.privateHoistedModulesDir),
  })
  const promises: Array<Promise<void>> = []
  for (const [hoistedDepNodeId, pkgAliases] of hoistedDependenciesByNodeId.entries()) {
    promises.push((async () => {
      const node = opts.graph[hoistedDepNodeId]
      if (node == null) {
        // This dependency is probably a skipped optional dependency.
        hoistLogger.debug({ hoistFailedFor: hoistedDepNodeId })
        return
      }
      const depLocation = node.dir
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
  opts: { virtualStoreDir: string, internalPnpmDir: string },
  depLocation: string,
  dest: string
): Promise<void> {
  return withFileLockRetryAsync(() => symlinkHoistedDependencyOnce(opts, depLocation, dest))
}

async function symlinkHoistedDependencyOnce (
  opts: { virtualStoreDir: string, internalPnpmDir: string },
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
    existingSymlink = await withFileLockRetryAsync(() => resolveLinkTarget(dest))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return createHoistedDependencyLink(depLocation, dest)
    }
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'EINVAL') throw err
    hoistLogger.debug({
      skipped: dest,
      reason: 'a directory is present at the target location',
    })
    return
  }
  if (!isSubdir(opts.virtualStoreDir, existingSymlink) && !isSubdir(opts.internalPnpmDir, existingSymlink)) {
    hoistLogger.debug({
      skipped: dest,
      existingSymlink,
      reason: 'an external symlink is present at the target location',
    })
    return
  }
  try {
    await fs.promises.unlink(dest)
  } catch (err: unknown) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
  }
  await createHoistedDependencyLink(depLocation, dest)
}

async function createHoistedDependencyLink (depLocation: string, dest: string): Promise<void> {
  let retries = 0
  while (true) {
    try {
      // eslint-disable-next-line no-await-in-loop
      await symlinkDir(depLocation, dest, { overwrite: false })
      break
    } catch (err: unknown) {
      if (!util.types.isNativeError(err) || !('code' in err) || (err.code !== 'EEXIST' && err.code !== 'EISDIR')) throw err
      let winningTarget: string
      try {
        // eslint-disable-next-line no-await-in-loop
        winningTarget = await withFileLockRetryAsync(() => resolveLinkTarget(dest))
      } catch (readError: unknown) {
        retries += 1
        if (util.types.isNativeError(readError) && 'code' in readError) {
          if (readError.code === 'ENOENT' && retries <= 100) continue
          if (readError.code === 'EINVAL') {
            // macOS can report EINVAL when a concurrent unlink interrupts readlink.
            try {
              // eslint-disable-next-line no-await-in-loop
              const stat = await fs.promises.lstat(dest)
              // eslint-disable-next-line no-await-in-loop
              if ((stat.isSymbolicLink() || await mayBeJunctionInCreation(dest, stat)) && retries <= 100) {
                // eslint-disable-next-line no-await-in-loop
                await delay(1)
                continue
              }
            } catch (statError: unknown) {
              if (util.types.isNativeError(statError) && 'code' in statError && statError.code === 'ENOENT' && retries <= 100) continue
            }
          }
        }
        throw err
      }
      if (path.relative(depLocation, winningTarget) !== '') throw err
      break
    }
  }
  linkLogger.debug({ target: dest, link: depLocation })
}

/**
 * A junction is created as an empty directory that gets its reparse point
 * afterwards, so a concurrent hoist can find an empty plain directory in its
 * place for a moment. A junction completed after `stat` was read lists its
 * target's entries, so a non-empty directory is checked again.
 */
async function mayBeJunctionInCreation (dest: string, stat: fs.Stats): Promise<boolean> {
  if (process.platform !== 'win32' || !stat.isDirectory()) return false
  if ((await fs.promises.readdir(dest)).length === 0) return true
  return (await fs.promises.lstat(dest)).isSymbolicLink()
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
