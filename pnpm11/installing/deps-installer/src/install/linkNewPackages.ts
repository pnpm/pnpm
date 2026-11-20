import { promises as fs } from 'node:fs'
import path from 'node:path'

import { statsLogger } from '@pnpm/core-loggers'
import type { DepsStateCache } from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import type {
  DependenciesGraph,
  DependenciesGraphNode,
} from '@pnpm/installing.deps-resolver'
import { removeObsoleteDependency } from '@pnpm/installing.linking.modules-cleaner'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.fs'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import type { StoreController, TarballResolution } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { symlinkAllModules } from '@pnpm/worker'
import pLimit from 'p-limit'
import { pathExists } from 'path-exists'
import { difference, equals, isEmpty, pickBy, props } from 'ramda'

import { linkAllPkgs } from './linkAllPkgs.js'

const brokenModulesLogger = logger('_broken_node_modules')

export interface LinkNewPackagesOptions {
  allowBuild?: AllowBuild
  deferDependencyBuilds: boolean
  depsStateCache: DepsStateCache
  disableRelinkLocalDirDeps?: boolean
  enableGlobalVirtualStore: boolean
  force: boolean
  optional: boolean
  ignoreScripts: boolean
  lockfileDir: string
  sideEffectsCacheRead: boolean
  remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
  pnprServer?: string
  configByUri: Record<string, RegistryConfig>
  symlink: boolean
  skipped: Set<DepPath>
  storeController: StoreController
  supportedArchitectures?: SupportedArchitectures
  virtualStoreDir: string
}

interface LinkNewPackagesResult {
  newDepPaths: DepPath[]
  added: number
}

type ModulesLinkJob = Pick<DependenciesGraphNode, 'children' | 'modules' | 'name' | 'optionalDependencies'> & {
  removedAliases?: string[]
}

interface LockfilePair {
  currentLockfile: LockfileObject
  wantedLockfile: LockfileObject
}

export async function linkNewPackages (
  currentLockfile: LockfileObject,
  wantedLockfile: LockfileObject,
  depGraph: DependenciesGraph,
  opts: LinkNewPackagesOptions
): Promise<LinkNewPackagesResult> {
  const wantedRelDepPaths = difference(Object.keys(wantedLockfile.packages ?? {}) as DepPath[], Array.from(opts.skipped))

  const newDepPathsSet = opts.force
    ? new Set(
      wantedRelDepPaths
        // when installing a new package, not all the nodes are analyzed
        // just skip the ones that are in the lockfile but were not analyzed
        .filter((depPath) => depGraph[depPath])
    )
    : await selectNewFromWantedDeps(wantedRelDepPaths, currentLockfile, depGraph)

  const added = newDepPathsSet.size
  statsLogger.debug({
    added,
    prefix: opts.lockfileDir,
  })

  const existingWithUpdatedDeps = opts.force
    ? []
    : await findExistingWithUpdatedDeps(wantedRelDepPaths, {
      depGraph,
      lockfiles: { currentLockfile, wantedLockfile },
      newDepPathsSet,
      opts,
    })

  if (!newDepPathsSet.size && (existingWithUpdatedDeps.length === 0)) return { newDepPaths: [], added }

  const newDepPaths = Array.from(newDepPathsSet)
  await linkPackagesAndModules(props<DepPath, DependenciesGraphNode>(newDepPaths, depGraph), {
    depGraph,
    existingWithUpdatedDeps,
    lockfiles: { currentLockfile, wantedLockfile },
    opts,
  })

  return { newDepPaths, added }
}

interface ExistingPackagesContext {
  depGraph: DependenciesGraph
  lockfiles: LockfilePair
  newDepPathsSet: Set<DepPath>
  opts: LinkNewPackagesOptions
}

/** The already-linked packages whose children changed, as jobs relinking only those children. */
async function findExistingWithUpdatedDeps (
  wantedRelDepPaths: DepPath[],
  context: ExistingPackagesContext
): Promise<ModulesLinkJob[]> {
  const currentPackages = context.lockfiles.currentLockfile.packages
  const wantedPackages = context.lockfiles.wantedLockfile.packages
  if (currentPackages == null || wantedPackages == null) return []
  const existingWithUpdatedDeps: ModulesLinkJob[] = []
  await Promise.all(wantedRelDepPaths.map((depPath) => limitModulesDirReads(async () => {
    const currentSnapshot = currentPackages[depPath]
    if (!currentSnapshot || !childrenMayHaveChanged(currentSnapshot, wantedPackages[depPath])) return
    const job = await getUpdatedChildrenJob(depPath, { currentSnapshot, wantedSnapshot: wantedPackages[depPath] }, context)
    if (job != null) existingWithUpdatedDeps.push(job)
  })))
  return existingWithUpdatedDeps
}

function childrenMayHaveChanged (currentSnapshot: PackageSnapshot, wantedSnapshot: PackageSnapshot): boolean {
  return !equals(currentSnapshot.dependencies, wantedSnapshot.dependencies) ||
    !isEmpty(currentSnapshot.optionalDependencies ?? {}) ||
    !isEmpty(wantedSnapshot.optionalDependencies ?? {})
}

async function getUpdatedChildrenJob (
  depPath: DepPath,
  { currentSnapshot, wantedSnapshot }: { currentSnapshot: PackageSnapshot, wantedSnapshot: PackageSnapshot },
  { depGraph, newDepPathsSet, opts }: ExistingPackagesContext
): Promise<ModulesLinkJob | undefined> {
  // TODO: come up with a test that triggers the usecase of depGraph[depPath] undefined
  // see related issue: https://github.com/pnpm/pnpm/issues/870
  if (!depGraph[depPath] || newDepPathsSet.has(depPath)) return undefined
  const { actualChildrenChanged, removedAliases: actualRemovedAliases } = await getActualChildrenDiff(
    depGraph[depPath],
    depGraph,
    opts.lockfileDir,
    opts.optional
  )
  const depNode = depGraph[depPath]
  if (actualChildrenChanged) {
    return {
      children: depNode.children,
      modules: depNode.modules,
      name: depNode.name,
      optionalDependencies: depNode.optionalDependencies,
      removedAliases: actualRemovedAliases,
    }
  }
  const { changedChildren, removedAliases } = getChangedChildren({
    currentDependencies: currentSnapshot.dependencies,
    currentOptionalDependencies: currentSnapshot.optionalDependencies,
    wantedDependencies: wantedSnapshot.dependencies,
    wantedOptionalDependencies: wantedSnapshot.optionalDependencies,
    allChildren: depNode.children,
  })
  if (isEmpty(changedChildren) && removedAliases.length === 0) return undefined
  return {
    children: changedChildren,
    modules: depNode.modules,
    name: depNode.name,
    optionalDependencies: depNode.optionalDependencies,
    removedAliases,
  }
}

async function linkPackagesAndModules (
  newPkgs: DependenciesGraphNode[],
  { depGraph, existingWithUpdatedDeps, lockfiles, opts }: {
    depGraph: DependenciesGraph
    existingWithUpdatedDeps: ModulesLinkJob[]
    lockfiles: LockfilePair
    opts: LinkNewPackagesOptions
  }
): Promise<void> {
  const newModuleLinks: ModulesLinkJob[] = newPkgs.map((depNode) => toNewModulesLinkJob(depNode, lockfiles))

  await Promise.all(newPkgs.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
  await Promise.all([
    !opts.symlink
      ? Promise.resolve()
      : linkAllModules([...newModuleLinks, ...existingWithUpdatedDeps], depGraph, {
        lockfileDir: opts.lockfileDir,
        optional: opts.optional,
      }),
    linkAllPkgs(opts.storeController, newPkgs, {
      allowBuild: opts.allowBuild,
      depGraph,
      depsStateCache: opts.depsStateCache,
      deferDependencyBuilds: opts.deferDependencyBuilds,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
      force: opts.force,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      nodeVersion: findLockedRootNodeRuntime(lockfiles.wantedLockfile)?.version,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      remoteSideEffectsCache: opts.remoteSideEffectsCache,
      pnprServer: opts.pnprServer,
      configByUri: opts.configByUri,
      supportedArchitectures: opts.supportedArchitectures,
    }),
  ])
}

function toNewModulesLinkJob (
  depNode: DependenciesGraphNode,
  { currentLockfile, wantedLockfile }: LockfilePair
): ModulesLinkJob {
  const currentSnapshot = currentLockfile.packages?.[depNode.depPath]
  const wantedSnapshot = wantedLockfile.packages?.[depNode.depPath]
  if (currentSnapshot == null || wantedSnapshot == null) return depNode
  const { removedAliases } = getChangedChildren({
    currentDependencies: currentSnapshot.dependencies,
    currentOptionalDependencies: currentSnapshot.optionalDependencies,
    wantedDependencies: wantedSnapshot.dependencies,
    wantedOptionalDependencies: wantedSnapshot.optionalDependencies,
    allChildren: depNode.children,
  })
  return { ...depNode, removedAliases: removedAliases.filter((alias) => alias !== depNode.name) }
}

async function selectNewFromWantedDeps (
  wantedRelDepPaths: DepPath[],
  currentLockfile: LockfileObject,
  depGraph: DependenciesGraph
): Promise<Set<DepPath>> {
  const newDeps = new Set<DepPath>()
  const prevDeps = currentLockfile.packages ?? {}
  await Promise.all(
    wantedRelDepPaths.map(
      async (depPath) => {
        const depNode = depGraph[depPath]
        if (!depNode) return
        const prevDep = prevDeps[depPath]
        if (
          prevDep &&
          // Local file should always be treated as a new dependency
          // https://github.com/pnpm/pnpm/issues/5381
          depNode.resolution.type !== 'directory' &&
          (depNode.resolution as TarballResolution).integrity === (prevDep.resolution as TarballResolution).integrity
        ) {
          if (await pathExists(depNode.dir)) {
            return
          }
          brokenModulesLogger.debug({
            missing: depNode.dir,
          })
        }
        newDeps.add(depPath)
      }
    )
  )
  return newDeps
}

const limitModulesDirReads = pLimit(16)

async function linkAllModules (
  depNodes: ModulesLinkJob[],
  depGraph: DependenciesGraph,
  opts: {
    lockfileDir: string
    optional: boolean
  }
): Promise<void> {
  await Promise.all(depNodes.flatMap((depNode) => (depNode.removedAliases ?? []).map(async (alias) => limitModulesDirReads(async () => removeObsoleteDependency(depNode.modules, alias)))))
  await symlinkAllModules({
    deps: depNodes.map((depNode) => {
      return {
        children: getChildrenPaths(depNode, depGraph, opts.lockfileDir, opts.optional),
        modules: depNode.modules,
        name: depNode.name,
      }
    }),
  })
}

function getChangedChildren (
  opts: {
    currentDependencies: Record<string, string> | undefined
    currentOptionalDependencies: Record<string, string> | undefined
    wantedDependencies: Record<string, string> | undefined
    wantedOptionalDependencies: Record<string, string> | undefined
    allChildren: Record<string, DepPath>
  }
): { changedChildren: Record<string, DepPath>, removedAliases: string[] } {
  const { currentOptionalDependencies, wantedOptionalDependencies, allChildren } = opts
  // Use null-prototype maps so a child literally named `constructor`,
  // `toString`, `__proto__`, etc. is treated as a normal alias instead
  // of colliding with an inherited `Object.prototype` key during the
  // `in`/index lookups below.
  const currentChildren: Record<string, string> = Object.assign(Object.create(null), opts.currentDependencies, currentOptionalDependencies)
  const wantedChildren: Record<string, string> = Object.assign(Object.create(null), opts.wantedDependencies, wantedOptionalDependencies)
  const changedChildren: Record<string, DepPath> = {}
  for (const [alias, wantedChildDepPath] of Object.entries(wantedChildren)) {
    const optionalityChanged = hasOwn(wantedOptionalDependencies, alias) !== hasOwn(currentOptionalDependencies, alias)
    if (currentChildren[alias] === wantedChildDepPath && !optionalityChanged) continue
    const resolvedChildDepPath = hasOwn(allChildren, alias) ? allChildren[alias] : undefined
    if (resolvedChildDepPath != null) {
      changedChildren[alias] = resolvedChildDepPath
    }
  }
  const removedAliases = Object.keys(currentChildren).filter((alias) => !hasOwn(wantedChildren, alias))
  return { changedChildren, removedAliases }
}

function hasOwn (obj: Record<string, unknown> | undefined, key: string): boolean {
  return obj != null && Object.hasOwn(obj, key)
}

async function getActualChildrenDiff (
  depNode: ModulesLinkJob,
  depGraph: DependenciesGraph,
  lockfileDir: string,
  optional: boolean
): Promise<{ actualChildrenChanged: boolean, removedAliases: string[] }> {
  if (depNode.optionalDependencies.size === 0) {
    return { actualChildrenChanged: false, removedAliases: [] }
  }
  const currentAliases = new Set((await readModulesDir(depNode.modules) ?? []).filter((alias) => alias !== depNode.name))
  const nextAliases = new Set(Object.keys(getChildrenPaths(depNode, depGraph, lockfileDir, optional)))
  const removedAliases = Array.from(currentAliases).filter((alias) => !nextAliases.has(alias))
  const actualChildrenChanged = removedAliases.length > 0 ||
    Array.from(nextAliases).some((alias) => !currentAliases.has(alias))
  return { actualChildrenChanged, removedAliases }
}

function getChildrenPaths (
  depNode: ModulesLinkJob,
  depGraph: DependenciesGraph,
  lockfileDir: string,
  optional: boolean
): Record<string, string> {
  const children = optional
    ? depNode.children
    : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)
  const childrenPaths: Record<string, string> = {}
  for (const [alias, childDepPath] of Object.entries(children ?? {})) {
    if (alias === depNode.name) continue
    const childPath = resolveChildPath(childDepPath, { depNode, depGraph, lockfileDir })
    if (childPath != null) childrenPaths[alias] = childPath
  }
  return childrenPaths
}

/** Where the `node_modules` link to a child points, or `undefined` for a child that is not installed. */
function resolveChildPath (
  childDepPath: DepPath,
  { depNode, depGraph, lockfileDir }: { depNode: ModulesLinkJob, depGraph: DependenciesGraph, lockfileDir: string }
): string | undefined {
  const packageRootLinkTarget = dp.packageRootLinkTarget(childDepPath)
  if (packageRootLinkTarget != null) {
    return path.join(depNode.modules, depNode.name, packageRootLinkTarget)
  }
  if (childDepPath.startsWith('link:')) {
    return path.resolve(lockfileDir, childDepPath.slice(5))
  }
  const pkg = depGraph[childDepPath]
  if (!pkg || !pkg.installable && pkg.optional) return undefined
  return pkg.dir
}
