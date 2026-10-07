import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { prepareWorkspaceModulesDir, validateWorkspaceModulesDir } from '@pnpm/fs.symlink-dependency'
import type { HoistedDependencies, ProjectId } from '@pnpm/types'
import { resolveLinkTarget } from 'resolve-link-target'

import { createGetAliasHoistType } from './createGetAliasHoistType.js'
import { getTrustedRoot, hoistLogger, selectHoistedModulesDir } from './hoistedModulesDirs.js'
import { symlinkHoistedDependency } from './symlinkHoistedDependency.js'
import type { DependenciesGraph, DirectDependenciesByImporterId, HoistedModulesDirs, HoistedWorkspaceProject, HoistType } from './types.js'
import { findConflictingWorkspaceProjectIds, type OccupiedAliases, type WorkspacePlacementCandidate } from './workspaceAliasConflicts.js'

type WorkspaceRollback = (() => Promise<void>) | void

export interface HoistWorkspacePackagesOpts<NodeId extends string> {
  beforeWorkspaceLinks?: (hoistedDependencies: HoistedDependencies) => Promise<WorkspaceRollback>
  directDepsByImporterId: DirectDependenciesByImporterId<NodeId>
  /** Non-link direct dependency aliases of every project, including packages omitted from an incremental or filtered graph. */
  directDependencyAliases?: Iterable<string>
  graph: DependenciesGraph<NodeId>
  hoistedWorkspacePackages?: Record<ProjectId, HoistedWorkspaceProject>
  occupiedAliases?: OccupiedAliases
  privateHoistedModulesDir: string
  privateHoistPattern: string[]
  publicHoistedModulesDir: string
  publicHoistPattern: string[]
  virtualStoreDir: string
}

type WorkspacePlacement = readonly [ProjectId, HoistedWorkspaceProject, HoistType, string]

/**
 * Symlinks the workspace projects that the hoist patterns select.
 *
 * Every named project of the workspace is a candidate, not only the ones some other
 * project depends on, so this pass reads the projects rather than the dependency
 * graph. That also makes it independent of the graph: it is the whole of the work
 * for a workspace that installs nothing from a registry, and an install that changes
 * no dependency can run it on its own, without walking the graph again.
 *
 * A project loses its alias to a non-link direct dependency of any project,
 * including packages omitted from an incremental or filtered graph.
 */
export async function hoistWorkspacePackages<NodeId extends string> (opts: HoistWorkspacePackagesOpts<NodeId>): Promise<HoistedDependencies> {
  if (opts.hoistedWorkspacePackages == null) {
    await opts.beforeWorkspaceLinks?.({})
    return {}
  }
  const placementCandidates = selectWorkspacePlacementCandidates(opts, opts.hoistedWorkspacePackages)
  const hoistedDependencies = Object.fromEntries(placementCandidates.map(([projectId, { name }, hoistType]) => [
    projectId,
    { [name]: hoistType },
  ])) as HoistedDependencies
  const rollback = await opts.beforeWorkspaceLinks?.(hoistedDependencies)
  const placements = await prepareWorkspacePlacements(placementCandidates, opts, rollback)
  await linkWorkspacePlacements(placements, opts, rollback)
  return hoistedDependencies
}

function selectWorkspacePlacementCandidates<NodeId extends string> (
  opts: HoistWorkspacePackagesOpts<NodeId>,
  hoistedWorkspacePackages: Record<ProjectId, HoistedWorkspaceProject>
): WorkspacePlacementCandidate[] {
  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)
  const aliasesTakenByDependencies = collectAliasesTakenByDependencies(opts.graph, opts.directDepsByImporterId)
  for (const alias of opts.directDependencyAliases ?? []) aliasesTakenByDependencies.add(alias.toLowerCase())
  const placementCandidates: WorkspacePlacementCandidate[] = []
  for (const [projectId, project] of Object.entries(hoistedWorkspacePackages) as Array<[ProjectId, HoistedWorkspaceProject]>) {
    const { name } = project
    const hoistType = getAliasHoistType(name)
    if (!hoistType || aliasesTakenByDependencies.has(name.toLowerCase())) continue
    aliasesTakenByDependencies.add(name.toLowerCase())
    placementCandidates.push([projectId, project, hoistType])
  }
  const conflictingProjectIds = findConflictingWorkspaceProjectIds(placementCandidates, opts.occupiedAliases)
  return placementCandidates.filter(([projectId]) => !conflictingProjectIds.has(projectId))
}

function collectAliasesTakenByDependencies<NodeId extends string> (
  graph: DependenciesGraph<NodeId>,
  directDepsByImporterId: DirectDependenciesByImporterId<NodeId>
): Set<string> {
  const aliasesTakenByDependencies = new Set<string>()
  for (const directDeps of Object.values(directDepsByImporterId)) {
    for (const [alias, nodeId] of directDeps.entries()) {
      if (graph[nodeId] == null) continue
      aliasesTakenByDependencies.add(alias.toLowerCase())
    }
  }
  return aliasesTakenByDependencies
}

async function prepareWorkspacePlacements (
  placementCandidates: WorkspacePlacementCandidate[],
  dirs: HoistedModulesDirs,
  rollback: WorkspaceRollback
): Promise<WorkspacePlacement[]> {
  try {
    return await Promise.all(placementCandidates.map(async ([projectId, project, hoistType]) => {
      const targetDir = selectHoistedModulesDir(hoistType, dirs)
      const trustedRoot = getTrustedRoot(dirs.publicHoistedModulesDir, targetDir)
      const destination = await prepareWorkspaceModulesDir(targetDir, project.name, trustedRoot)
      return [projectId, project, hoistType, destination] as const
    }))
  } catch (error: unknown) {
    await runWorkspaceRollback(rollback, dirs.privateHoistedModulesDir)
    throw error
  }
}

async function linkWorkspacePlacements (
  placements: WorkspacePlacement[],
  opts: { privateHoistedModulesDir: string, virtualStoreDir: string },
  rollback: WorkspaceRollback
): Promise<void> {
  const symlink = symlinkHoistedDependency.bind(null, {
    virtualStoreDir: opts.virtualStoreDir,
    installStateDir: path.dirname(opts.privateHoistedModulesDir),
  })
  const results = await Promise.allSettled(placements.map(async ([, { dir }, , destination]) => symlink(dir, destination)))
  const failure = findRejection(results)
  if (failure == null) return
  const cleanupResults = await Promise.allSettled(placements.map(async ([, { dir }, , destination], index) => {
    if (results[index].status !== 'fulfilled') return
    await removeWorkspaceLinkIfTarget(destination, dir)
  }))
  reportWorkspaceRollbackFailures(cleanupResults, opts.privateHoistedModulesDir)
  await runWorkspaceRollback(rollback, opts.privateHoistedModulesDir)
  throw failure.reason
}

interface StaleWorkspaceLink {
  alias: string
  destination: string
  modulesDir: string
  target: string
  trustedRoot: string
}

export async function pruneStaleWorkspaceHoists (
  previous: HoistedDependencies,
  next: HoistedDependencies,
  projectIds: Set<ProjectId>,
  privateHoistedModulesDir: string,
  publicHoistedModulesDir: string
): Promise<() => Promise<void>> {
  const staleLinks = await readStaleWorkspaceLinks({
    previous,
    next,
    projectIds,
    dirs: { privateHoistedModulesDir, publicHoistedModulesDir },
  })
  const removalResults = await Promise.allSettled(staleLinks.map(unlinkStaleWorkspaceLink))
  const failure = findRejection(removalResults)
  if (failure != null) {
    await restoreWorkspaceLinks(staleLinks.filter((_, index) => removalResults[index].status === 'fulfilled'))
    throw failure.reason
  }
  const parentCleanupResults = await Promise.allSettled(selectOneLinkPerParent(staleLinks).map(removeEmptyParentsOfWorkspaceLink))
  const parentCleanupFailure = findRejection(parentCleanupResults)
  if (parentCleanupFailure != null) {
    await restoreWorkspaceLinks(staleLinks)
    throw parentCleanupFailure.reason
  }
  return async () => restoreWorkspaceLinks(staleLinks)
}

async function readStaleWorkspaceLinks (opts: {
  previous: HoistedDependencies
  next: HoistedDependencies
  projectIds: Set<ProjectId>
  dirs: HoistedModulesDirs
}): Promise<StaleWorkspaceLink[]> {
  const staleLinks = await Promise.all(Array.from(opts.projectIds).flatMap((projectId) => {
    const nextAliases = opts.next[projectId]
    return Object.entries(opts.previous[projectId] ?? {}).flatMap(([alias, hoistType]) => {
      if (nextAliases?.[alias] === hoistType) return []
      return [readWorkspaceLink(alias, selectHoistedModulesDir(hoistType, opts.dirs), opts.dirs.publicHoistedModulesDir)]
    })
  }))
  return staleLinks.filter(link => link != null)
}

async function readWorkspaceLink (alias: string, modulesDir: string, publicHoistedModulesDir: string): Promise<StaleWorkspaceLink | undefined> {
  const trustedRoot = getTrustedRoot(publicHoistedModulesDir, modulesDir)
  const destination = await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
  let target: string
  try {
    const rawTarget = await fs.promises.readlink(destination)
    target = path.resolve(path.dirname(destination), rawTarget)
  } catch (error: unknown) {
    if (isMissingLinkError(error)) {
      return undefined
    }
    throw error
  }
  return { alias, destination, modulesDir, target, trustedRoot }
}

async function unlinkStaleWorkspaceLink ({ alias, destination, modulesDir, trustedRoot }: StaleWorkspaceLink): Promise<void> {
  await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
  await fs.promises.unlink(destination)
}

/**
 * Concurrent removals of one directory fail on Windows with `EPERM` while the
 * delete is pending. A workspace alias has at most a scope directory above it,
 * so links with distinct parents never remove the same directory.
 */
function selectOneLinkPerParent (links: StaleWorkspaceLink[]): StaleWorkspaceLink[] {
  const linksByParent = new Map<string, StaleWorkspaceLink>()
  for (const link of links) {
    const parent = path.dirname(link.destination)
    if (!linksByParent.has(parent)) linksByParent.set(parent, link)
  }
  return Array.from(linksByParent.values())
}

async function removeEmptyParentsOfWorkspaceLink ({ alias, destination, modulesDir, trustedRoot }: StaleWorkspaceLink): Promise<void> {
  await removeEmptyWorkspaceParents(modulesDir, alias, path.dirname(destination), trustedRoot)
}

async function restoreWorkspaceLinks (links: StaleWorkspaceLink[]): Promise<void> {
  await Promise.all(links.map(async ({ alias, destination, modulesDir, target, trustedRoot }) => {
    await prepareWorkspaceModulesDir(modulesDir, alias, trustedRoot)
    await fs.promises.symlink(target, destination, process.platform === 'win32' ? 'junction' : 'dir')
  }))
}

async function removeEmptyWorkspaceParents (modulesDir: string, alias: string, parent: string, trustedRoot: string): Promise<void> {
  if (parent === modulesDir) return
  await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
  try {
    await fs.promises.rmdir(parent)
  } catch (error: unknown) {
    if (isError(error) && 'code' in error && ['ENOENT', 'ENOTEMPTY', 'EEXIST'].includes(String(error.code))) return
    throw error
  }
  await removeEmptyWorkspaceParents(modulesDir, alias, path.dirname(parent), trustedRoot)
}

async function runWorkspaceRollback (rollback: WorkspaceRollback, prefix: string): Promise<void> {
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

function findRejection<Value> (results: Array<PromiseSettledResult<Value>>): PromiseRejectedResult | undefined {
  return results.find((result): result is PromiseRejectedResult => result.status === 'rejected')
}

async function removeWorkspaceLinkIfTarget (destination: string, target: string): Promise<void> {
  let resolvedTarget: string
  try {
    resolvedTarget = await resolveLinkTarget(destination)
  } catch (error: unknown) {
    if (isMissingLinkError(error)) return
    throw error
  }
  if (resolvedTarget === target) await fs.promises.unlink(destination)
}

function isMissingLinkError (error: unknown): boolean {
  return isError(error) && 'code' in error && (error.code === 'ENOENT' || error.code === 'EINVAL')
}
