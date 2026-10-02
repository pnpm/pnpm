import path from 'node:path'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { statsLogger } from '@pnpm/core-loggers'
import {
  type DependenciesGraphNode,
  lockfileToDepGraph,
  type LockfileToDepGraphOptions,
} from '@pnpm/deps.graph-builder'
import { filterLockfileByImportersAndEngine } from '@pnpm/lockfile.filtering'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { writePnpFile } from '@pnpm/lockfile.to-pnp'
import {
  type AllowBuild,
  DEPENDENCIES_FIELDS,
  type DepPath,
  type ProjectId,
} from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'
import { equals } from 'ramda'
import { realpathMissing } from 'realpath-missing'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { pickMaterializedImporterIds } from './currentLockfileImporters.js'
import { lockfileToHoistedDepGraph } from './lockfileToHoistedDepGraph.js'
import type { Project } from './types.js'

export async function createHeadlessDepGraph (ctx: HeadlessContext): Promise<HeadlessDepGraph> {
  const { opts } = ctx
  const initialImporterIds = pickInitialImporterIds(ctx)
  const { lockfile: filteredLockfile, selectedImporterIds: importerIds, requiredDepPaths } = filterLockfileByImportersAndEngine(ctx.wantedLockfile, initialImporterIds, ctx.filterOpts)
  if (opts.excludeLinksFromLockfile) {
    addLinkedDependenciesToLockfile(filteredLockfile, ctx.selectedProjects)
  }
  addDeeplyLinkedProjects(ctx.selectedProjects, opts.allProjects, { initialImporterIds, importerIds })

  if (opts.enableGlobalVirtualStore) {
    opts.allowBuilds ??= {}
  }
  const allowBuild = createAllowBuildFunction(opts)
  const lockfileToDepGraphOpts = await createLockfileToDepGraphOptions(ctx, { allowBuild, importerIds, requiredDepPaths })
  const depGraph = await (
    opts.nodeLinker === 'hoisted'
      ? lockfileToHoistedDepGraph(filteredLockfile, ctx.currentLockfile, lockfileToDepGraphOpts)
      : lockfileToDepGraph(filteredLockfile, opts.force ? null : ctx.currentLockfile, lockfileToDepGraphOpts)
  )
  // `.pnp.cjs` is a project-level resolution artifact, so it follows the
  // same rule as the importer links and the package map above.
  if (opts.enablePnp && !ctx.skipPostImportLinking) {
    await writePnpFileOfProjects(ctx, filteredLockfile)
  }
  const depNodes = Object.values(depGraph.graph)
  markMaterializedNodesBuilt(depNodes)
  const added = depNodes.filter(({ fetching }) => fetching).length
  statsLogger.debug({
    added,
    prefix: ctx.lockfileDir,
  })
  return {
    ...depGraph,
    added,
    allowBuild,
    depNodes,
    filteredLockfile,
    importerIds,
    includeUnchangedDeps: lockfileToDepGraphOpts.includeUnchangedDeps === true,
  }
}

/**
 * Under `nodeLinker: hoisted` all projects share one node_modules, and the
 * install removes whatever the new hoisted tree leaves out. A selected install
 * therefore also keeps each project the previous install materialized a
 * package for. A project no install materialized stays out.
 */
function pickInitialImporterIds (ctx: HeadlessContext): ProjectId[] {
  const wantedImporterIds = Object.keys(ctx.wantedLockfile.importers) as ProjectId[]
  if (ctx.opts.ignorePackageManifest === true) return wantedImporterIds
  const selectedIds = ctx.selectedProjects.map(({ id }) => id)
  if (ctx.opts.nodeLinker !== 'hoisted' || ctx.currentLockfile == null) return selectedIds
  return Array.from(new Set([...selectedIds, ...pickMaterializedImporterIds(ctx.currentLockfile, wantedImporterIds)]))
}

function addLinkedDependenciesToLockfile (lockfile: LockfileObject, projects: Project[]): void {
  for (const { id, manifest, rootDir } of projects) {
    const importer = lockfile.importers[id]
    if (!importer) continue
    for (const depType of DEPENDENCIES_FIELDS) {
      importer[depType] = {
        ...importer[depType],
        ...getLinkSpecsRelativeTo(manifest[depType], rootDir),
      }
    }
  }
}

function getLinkSpecsRelativeTo (specs: Record<string, string> | undefined, rootDir: string): Record<string, string> {
  const linkSpecs: Record<string, string> = {}
  for (const [depName, spec] of Object.entries(specs ?? {})) {
    if (!spec.startsWith('link:')) continue
    const linkPath = spec.substring(5)
    linkSpecs[depName] = path.isAbsolute(linkPath) ? `link:${path.relative(rootDir, linkPath)}` : spec
  }
  return linkSpecs
}

/**
 * Adds the projects that the lockfile filter found through deeply linked
 * workspace projects to the selected projects.
 */
function addDeeplyLinkedProjects (
  selectedProjects: Project[],
  allProjects: Record<string, Project>,
  { initialImporterIds, importerIds }: { initialImporterIds: ProjectId[], importerIds: ProjectId[] }
): void {
  const initialImporterIdSet = new Set(initialImporterIds)
  const missingIds = importerIds.filter((importerId) => !initialImporterIdSet.has(importerId))
  if (missingIds.length === 0) return
  for (const project of Object.values(allProjects)) {
    if (missingIds.includes(project.id)) {
      selectedProjects.push(project)
    }
  }
}

async function createLockfileToDepGraphOptions (
  ctx: HeadlessContext,
  selection: {
    allowBuild: AllowBuild | undefined
    importerIds: ProjectId[]
    requiredDepPaths: Set<DepPath>
  }
): Promise<LockfileToDepGraphOptions> {
  const { opts } = ctx
  return {
    ...opts,
    ...installabilityUnderForce(opts),
    allowBuild: selection.allowBuild,
    importerIds: selection.importerIds,
    lockfileDir: ctx.lockfileDir,
    rootImporterId: opts.nodeLinker === 'hoisted'
      ? await findImporterOwningRootModulesDir(ctx.selectedProjects, selection.importerIds, ctx.rootModulesDir)
      : undefined,
    requiredDepPaths: selection.requiredDepPaths,
    skipped: ctx.skipped,
    virtualStoreDir: ctx.virtualStoreDir,
    nodeVersion: ctx.currentEngine.nodeVersion,
    pnpmVersion: ctx.currentEngine.pnpmVersion,
    supportedArchitectures: opts.supportedArchitectures,
    omitResolvedProgress: opts.omitResolvedProgress,
    includeUnchangedDeps: shouldIncludeUnchangedDeps(ctx),
  } as LockfileToDepGraphOptions
}

function shouldIncludeUnchangedDeps ({ opts, currentLockfile, wantedLockfile }: HeadlessContext): boolean {
  return (!equals(opts.currentHoistPattern ?? [], opts.hoistPattern ?? [])) ||
    (!equals(opts.currentPublicHoistPattern ?? [], opts.publicHoistPattern ?? [])) ||
    (opts.enableGlobalVirtualStore === true && !equals(opts.modulesFile?.allowBuilds ?? {}, opts.allowBuilds ?? {})) ||
    lockfileRemovesPackages(currentLockfile, wantedLockfile)
}

/**
 * Whether moving from the installed state to the wanted lockfile removes any
 * package. A removal changes the hoist eligibility of packages that stay, and
 * the incremental graph of an install that only removes packages is empty, so
 * the graph must include the unchanged packages for the hoist layer to be
 * recomputed. Compared against the full wanted lockfile, not the filtered
 * one, so a filtered install is not mistaken for a removal.
 */
function lockfileRemovesPackages (currentLockfile: LockfileObject | null, wantedLockfile: LockfileObject): boolean {
  if (currentLockfile?.packages == null) return false
  const wantedPackages = wantedLockfile.packages
  if (wantedPackages == null) return Object.keys(currentLockfile.packages).length > 0
  return Object.keys(currentLockfile.packages).some((depPath) => wantedPackages[depPath as DepPath] == null)
}

// pnpm deploy points the deployed project's node_modules at the root one and
// leaves the root project out of the install.
async function findImporterOwningRootModulesDir (
  projects: Array<Pick<Project, 'id' | 'modulesDir' | 'rootDir'>>,
  importerIds: ProjectId[],
  rootModulesDir: string
): Promise<ProjectId | undefined> {
  const importerIdsSet = new Set(importerIds)
  if (importerIdsSet.has('.' as ProjectId)) return undefined
  for (const { id, modulesDir, rootDir } of projects) {
    if (!importerIdsSet.has(id)) continue
    // eslint-disable-next-line no-await-in-loop -- the first project in order whose modules dir is the root one wins, so later ones need not be resolved
    if (await realpathMissing(pathAbsolute(modulesDir, rootDir)) === rootModulesDir) return id
  }
  return undefined
}

async function writePnpFileOfProjects (ctx: HeadlessContext, filteredLockfile: LockfileObject): Promise<void> {
  const importerNames = Object.fromEntries(
    ctx.selectedProjects.map(({ manifest, id }) => [id, manifest.name ?? id])
  )
  await writePnpFile(filteredLockfile, {
    importerNames,
    lockfileDir: ctx.lockfileDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.opts.virtualStoreDirMaxLength,
    registriesByScope: ctx.opts.registriesByScope,
  })
}

// A node the graph carries without a fetch was already materialized by an
// earlier install. It is in the graph so hoisting can see it; the build
// step must not run its scripts or re-apply its patch.
function markMaterializedNodesBuilt (depNodes: DependenciesGraphNode[]): void {
  for (const depNode of depNodes) {
    if (depNode.fetching == null) depNode.isBuilt = true
  }
}
