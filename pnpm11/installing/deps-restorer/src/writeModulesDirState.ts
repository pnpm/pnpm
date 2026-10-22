import path from 'node:path'

import { LAYOUT_VERSION } from '@pnpm/constants'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { filterLockfileByImporters } from '@pnpm/lockfile.filtering'
import { isEmptyLockfile, type LockfileObject, writeCurrentLockfile, writeLockfiles } from '@pnpm/lockfile.fs'
import type { HoistedDependencies, IgnoredBuilds, ProjectId } from '@pnpm/types'
import { pick } from 'ramda'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { getInjectedDeps } from './extendProjectsWithTargetDirs.js'
import { linkBinsOfImporters } from './linkBinsOfImporters.js'

export interface InstallOutcome {
  ignoredBuilds: IgnoredBuilds | undefined
  newHoistedDependencies?: HoistedDependencies
}

/**
 * Links the bins of the importers and records the installation in the
 * modules manifest and the current lockfile.
 */
export async function writeModulesDirState (ctx: HeadlessContext, depGraph: HeadlessDepGraph, outcome: InstallOutcome): Promise<void> {
  const { opts } = ctx
  if (opts.enableModulesDir === false) return
  /** Skip linking and due to no project manifest */
  if (!ctx.skipPostImportLinking && !opts.ignorePackageManifest) {
    await linkBinsOfImporters(ctx, depGraph)
  }
  await writeModulesManifestOfInstall(ctx, depGraph, outcome)
  await writeLockfilesOfInstall(ctx, mergeIntoCurrentLockfile(ctx, depGraph))
}

const mergedCurrentLockfiles = new WeakMap<HeadlessDepGraph, LockfileObject>()

export function mergeIntoCurrentLockfile (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph
): LockfileObject {
  const cached = mergedCurrentLockfiles.get(depGraph)
  if (cached != null) return cached
  const merged = computeMergedCurrentLockfile(ctx, depGraph)
  mergedCurrentLockfiles.set(depGraph, merged)
  return merged
}

function computeMergedCurrentLockfile (
  { currentLockfile, wantedLockfile }: HeadlessContext,
  { filteredLockfile, importerIds }: HeadlessDepGraph
): LockfileObject {
  if (Object.keys(wantedLockfile.importers).every((id) => importerIds.includes(id as ProjectId))) {
    return filteredLockfile
  }
  const emptyImporters = Object.fromEntries(
    Object.keys(filteredLockfile.importers).map((id) => [id, { specifiers: {} }])
  ) as LockfileObject['importers']
  if (currentLockfile == null || isEmptyLockfile(currentLockfile)) {
    return {
      ...filteredLockfile,
      importers: {
        ...emptyImporters,
        ...pick(importerIds, filteredLockfile.importers),
      },
    }
  }
  const retainedPackages = retainedUnselectedPackages(currentLockfile, filteredLockfile, importerIds)
  return {
    ...filteredLockfile,
    importers: {
      ...emptyImporters,
      ...currentLockfile.importers,
      ...pick(importerIds, filteredLockfile.importers),
    },
    packages: {
      ...retainedPackages,
      ...filteredLockfile.packages,
    },
  }
}

const ALL_INCLUDED_DEPENDENCIES = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
} as const

function retainedUnselectedPackages (
  currentLockfile: LockfileObject,
  filteredLockfile: LockfileObject,
  importerIds: ProjectId[]
): LockfileObject['packages'] {
  const previouslyRecordedIds = importerIds.filter((id) => Object.hasOwn(currentLockfile.importers, id))
  const prevSelected = filterLockfileByImporters(currentLockfile, previouslyRecordedIds, {
    include: ALL_INCLUDED_DEPENDENCIES,
    failOnMissingDependencies: false,
    skipped: new Set(),
  })
  const nextPackages = filteredLockfile.packages ?? {}
  const droppedAnyPackage = Object.keys(prevSelected.packages ?? {}).some((depPath) => !(depPath in nextPackages))
  if (!droppedAnyPackage) {
    return currentLockfile.packages
  }
  const selectedSet = new Set(importerIds)
  const unselectedIds = (Object.keys(currentLockfile.importers) as ProjectId[]).filter((id) => !selectedSet.has(id))
  return filterLockfileByImporters(currentLockfile, unselectedIds, {
    include: ALL_INCLUDED_DEPENDENCIES,
    failOnMissingDependencies: false,
    skipped: new Set(),
  }).packages
}

async function writeModulesManifestOfInstall (ctx: HeadlessContext, depGraph: HeadlessDepGraph, outcome: InstallOutcome): Promise<void> {
  const { opts } = ctx
  const injectedDeps = getInjectedDeps(depGraph.injectionTargetsByDepPath, opts.lockfileDir)
  await writeModulesManifest(ctx.rootModulesDir, {
    hoistedDependencies: outcome.newHoistedDependencies!,
    hoistPattern: opts.hoistPattern,
    included: opts.include,
    injectedDeps,
    ignoredBuilds: outcome.ignoredBuilds,
    layoutVersion: LAYOUT_VERSION,
    hoistedLocations: depGraph.hoistedLocations,
    nodeLinker: opts.nodeLinker,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: opts.pendingBuilds,
    publicHoistPattern: opts.publicHoistPattern,
    prunedAt: opts.pruneVirtualStore === true || opts.prunedAt == null
      ? new Date().toUTCString()
      : opts.prunedAt,
    skipped: Array.from(ctx.skipped),
    storeDir: opts.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    allowBuilds: opts.allowBuilds,
    virtualStoreOnly: opts.virtualStoreOnly,
  })
}

async function writeLockfilesOfInstall ({ opts, rootModulesDir, wantedLockfile }: HeadlessContext, filteredLockfile: LockfileObject): Promise<void> {
  const currentLockfileDir = path.join(rootModulesDir, '.pnpm')
  if (opts.useLockfile) {
    // We need to write the wanted lockfile as well.
    // Even though it will only be changed if the workspace will have new projects with no dependencies.
    await writeLockfiles({
      wantedLockfileDir: opts.lockfileDir,
      currentLockfileDir,
      wantedLockfile,
      currentLockfile: filteredLockfile,
    })
  } else {
    await writeCurrentLockfile(currentLockfileDir, filteredLockfile)
  }
}
