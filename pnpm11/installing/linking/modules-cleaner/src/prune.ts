import { promises as fs } from 'node:fs'
import path from 'node:path'

import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { depPathToFilename } from '@pnpm/deps.path'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { filterLockfile, filterLockfileByImporters } from '@pnpm/lockfile.filtering'
import type {
  LockfileObject,
  PackageSnapshots,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import { packageIdFromSnapshot } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type DepPath,
  type HoistedDependencies,
  type ProjectId,
  type ProjectRootDir,
} from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { difference, equals, mergeAll, pickAll } from 'ramda'

import { removeDirectDependency, removeIfEmpty } from './removeDirectDependency.js'

export type PruneImporter = {
  binsDir: string
  id: ProjectId
  modulesDir: string
  pruneDirectDependencies?: boolean
  removePackages?: string[]
  rootDir: ProjectRootDir
}

export interface PruneOptions {
  dedupeDirectDeps?: boolean
  dryRun?: boolean
  include: { [dependenciesField in DependenciesField]: boolean }
  hoistedDependencies: HoistedDependencies
  hoistedModulesDir?: string
  publicHoistedModulesDir?: string
  wantedLockfile: LockfileObject
  currentLockfile: LockfileObject
  pruneStore?: boolean
  pruneVirtualStore?: boolean
  resolvePeersFromWorkspaceRoot?: boolean
  skipped: Set<DepPath>
  skipRuntimes?: boolean
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  lockfileDir: string
  storeController: StoreController
}

export async function prune (
  importers: PruneImporter[],
  opts: PruneOptions
): Promise<Set<string>> {
  const wantedLockfile = filterLockfile(opts.wantedLockfile, {
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped: opts.skipped,
    skipRuntimes: opts.skipRuntimes,
  })
  await pruneAllImportersDirectDeps(importers, wantedLockfile, opts)

  const orphanDepPaths = findOrphanDepPaths(importers, wantedLockfile, opts)
  if (!opts.dryRun) {
    await pruneHoistedDependencies(orphanDepPaths, opts)
    if (opts.pruneVirtualStore !== false) {
      await pruneVirtualStorePackages(orphanDepPaths, opts)
    }
  }

  return new Set(orphanDepPaths)
}

async function pruneAllImportersDirectDeps (
  importers: PruneImporter[],
  wantedLockfile: LockfileObject,
  opts: PruneOptions
): Promise<void> {
  const rootImporter = wantedLockfile.importers['.' as ProjectId] ?? {} as ProjectSnapshot
  const wantedRootPkgs = mergeDependencies(rootImporter)
  await Promise.all(importers.map(async (importer) => {
    await pruneImporterDirectDeps(importer, wantedLockfile, wantedRootPkgs, opts)
  }))
}

async function pruneImporterDirectDeps (
  importer: PruneImporter,
  wantedLockfile: LockfileObject,
  wantedRootPkgs: Record<string, string>,
  opts: PruneOptions
): Promise<void> {
  const { binsDir, id, modulesDir, pruneDirectDependencies, removePackages, rootDir } = importer
  const currentImporter = opts.currentLockfile.importers[id] || {} as ProjectSnapshot
  const currentPkgs = Object.entries(mergeDependencies(currentImporter))
  const wantedPkgs = mergeDependencies(wantedLockfile.importers[id])

  const allCurrentPackages = new Set(
    (pruneDirectDependencies === true || removePackages?.length)
      ? (await readModulesDir(modulesDir) ?? [])
      : []
  )
  const depsToRemove = collectDepsToRemove({
    allCurrentPackages,
    currentPkgs,
    dedupeDirectDeps: opts.dedupeDirectDeps,
    hoistedDependencies: opts.hoistedDependencies,
    id,
    pruneDirectDependencies,
    removePackages,
    wantedPkgs,
    wantedRootPkgs,
  })

  await removeDirectDependencies({
    binsDir,
    currentImporter,
    depsToRemove,
    dryRun: opts.dryRun,
    modulesDir,
    rootDir,
  })
}

interface CollectDepsToRemoveOptions {
  allCurrentPackages: Set<string>
  currentPkgs: Array<[string, string]>
  dedupeDirectDeps?: boolean
  hoistedDependencies: HoistedDependencies
  id: ProjectId
  pruneDirectDependencies?: boolean
  removePackages?: string[]
  wantedPkgs: Record<string, string>
  wantedRootPkgs: Record<string, string>
}

function collectDepsToRemove (opts: CollectDepsToRemoveOptions): Set<string> {
  const depsToRemove = new Set(
    (opts.removePackages ?? []).filter((pkg) => opts.allCurrentPackages.has(pkg))
  )
  for (const [depName, depVersion] of opts.currentPkgs) {
    if (shouldRemoveCurrentPkg(depName, depVersion, opts)) {
      depsToRemove.add(depName)
    }
  }
  if (opts.pruneDirectDependencies) {
    addUnwantedDirectDeps(depsToRemove, opts.allCurrentPackages, opts.wantedPkgs, opts.hoistedDependencies)
  }
  return depsToRemove
}

function shouldRemoveCurrentPkg (
  depName: string,
  depVersion: string,
  opts: Pick<CollectDepsToRemoveOptions, 'dedupeDirectDeps' | 'id' | 'wantedPkgs' | 'wantedRootPkgs'>
): boolean {
  const wantedVersion = opts.wantedPkgs[depName]
  if (!wantedVersion || wantedVersion !== depVersion) {
    return true
  }
  return Boolean(opts.dedupeDirectDeps && opts.id !== '.' && wantedVersion === opts.wantedRootPkgs[depName])
}

function addUnwantedDirectDeps (
  depsToRemove: Set<string>,
  allCurrentPackages: Set<string>,
  wantedPkgs: Record<string, string>,
  hoistedDependencies: HoistedDependencies
): void {
  if (allCurrentPackages.size === 0) return
  const publiclyHoistedDeps = getPubliclyHoistedDependencies(hoistedDependencies)
  for (const currentPackage of allCurrentPackages) {
    if (!wantedPkgs[currentPackage] && !publiclyHoistedDeps.has(currentPackage)) {
      depsToRemove.add(currentPackage)
    }
  }
}

interface RemoveDirectDepsOptions {
  binsDir: string
  currentImporter: ProjectSnapshot
  depsToRemove: Set<string>
  dryRun?: boolean
  modulesDir: string
  rootDir: ProjectRootDir
}

async function removeDirectDependencies (opts: RemoveDirectDepsOptions): Promise<void> {
  const { binsDir, currentImporter, depsToRemove, dryRun, modulesDir, rootDir } = opts
  const removedFromScopes = new Set<string>()
  await Promise.all(Array.from(depsToRemove).map(async (depName) => {
    const scope = getScopeFromPackageName(depName)
    if (scope) {
      removedFromScopes.add(scope)
    }
    return removeDirectDependency({
      dependenciesField: currentImporter.devDependencies?.[depName] != null && 'devDependencies' ||
        currentImporter.optionalDependencies?.[depName] != null && 'optionalDependencies' ||
        currentImporter.dependencies?.[depName] != null && 'dependencies' ||
        undefined,
      name: depName,
    }, {
      binsDir,
      dryRun,
      modulesDir,
      rootDir,
    })
  }))
  await Promise.all(Array.from(removedFromScopes).map((scope) => removeIfEmpty(path.join(modulesDir, scope))))
  try {
    await removeIfEmpty(modulesDir)
  } catch {
    // Ignored if lacking permission on some server setups
  }
}

function findOrphanDepPaths (
  importers: PruneImporter[],
  wantedLockfile: LockfileObject,
  opts: PruneOptions
): DepPath[] {
  const selectedImporterIds = importers.map((importer) => importer.id).sort()
  const currentPkgIdsByDepPaths = equals(selectedImporterIds, Object.keys(opts.wantedLockfile.importers))
    ? getPkgsDepPaths(opts.currentLockfile.packages ?? {}, opts.skipped)
    : getPkgsDepPathsOwnedOnlyByImporters(selectedImporterIds, opts.currentLockfile, opts)
  const wantedPkgIdsByDepPaths = getPkgsDepPaths(wantedLockfile.packages ?? {}, opts.skipped)

  const orphanDepPaths = (Object.keys(currentPkgIdsByDepPaths) as DepPath[])
    .filter((depPath: DepPath) => !wantedPkgIdsByDepPaths[depPath])
  const orphanPkgIds = new Set(orphanDepPaths.map(depPath => currentPkgIdsByDepPaths[depPath]))

  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: orphanPkgIds.size,
  })
  return orphanDepPaths
}

async function pruneHoistedDependencies (
  orphanDepPaths: DepPath[],
  opts: PruneOptions
): Promise<void> {
  if (
    orphanDepPaths.length === 0 ||
    opts.currentLockfile.packages == null ||
    (opts.hoistedModulesDir == null && opts.publicHoistedModulesDir == null)
  ) {
    return
  }
  const prefix = path.join(opts.virtualStoreDir, '../..')
  await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
    await removeHoistedEntriesForDepPath(orphanDepPath, prefix, opts)
  }))
}

async function removeHoistedEntriesForDepPath (
  orphanDepPath: DepPath,
  prefix: string,
  opts: PruneOptions
): Promise<void> {
  const hoistedEntries = opts.hoistedDependencies[orphanDepPath]
  if (!hoistedEntries) return

  await Promise.all(Object.entries(hoistedEntries).map(([alias, hoistType]) => {
    const modulesDir = hoistType === 'public'
      ? opts.publicHoistedModulesDir
      : opts.hoistedModulesDir
    if (!modulesDir) return undefined
    return removeDirectDependency({
      name: alias,
    }, {
      binsDir: path.join(modulesDir, '.bin'),
      modulesDir,
      muteLogs: true,
      rootDir: prefix as ProjectRootDir,
    })
  }))
  delete opts.hoistedDependencies[orphanDepPath]
}

async function pruneVirtualStorePackages (
  orphanDepPaths: DepPath[],
  opts: PruneOptions
): Promise<void> {
  const _tryRemovePkg = tryRemovePkg.bind(null, opts.lockfileDir, opts.virtualStoreDir)
  await Promise.all(
    orphanDepPaths
      .map((orphanDepPath) => depPathToFilename(orphanDepPath, opts.virtualStoreDirMaxLength))
      .map(async (orphanDepPath) => _tryRemovePkg(orphanDepPath))
  )
  const neededPkgs = new Set<string>(['node_modules'])
  for (const depPath of Object.keys(opts.wantedLockfile.packages ?? {})) {
    if (opts.skipped.has(depPath as DepPath)) continue
    neededPkgs.add(depPathToFilename(depPath, opts.virtualStoreDirMaxLength))
  }
  const availablePkgs = await readVirtualStoreDir(opts.virtualStoreDir, opts.lockfileDir)
  await Promise.all(
    availablePkgs
      .filter((availablePkg) => !neededPkgs.has(availablePkg))
      .map(async (orphanDepPath) => _tryRemovePkg(orphanDepPath))
  )
}

function getScopeFromPackageName (pkgName: string): string | undefined {
  if (pkgName[0] === '@') {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }
  return undefined
}

async function readVirtualStoreDir (virtualStoreDir: string, lockfileDir: string): Promise<string[]> {
  try {
    const entries = await fs.readdir(virtualStoreDir, { withFileTypes: true })
    return entries
      .filter(entry => !entry.isFile() || !isLockfileName(entry.name))
      .map(entry => entry.name)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') {
      logger.warn({
        error: err,
        message: `Failed to read virtualStoreDir at "${virtualStoreDir}"`,
        prefix: lockfileDir,
      })
    }
    return []
  }
}

function isLockfileName (name: string): boolean {
  return name === 'lock.yaml' || name.startsWith('lock.yaml.') ||
    (name.startsWith('.lock.yaml.') && name.endsWith('.tmp'))
}

async function tryRemovePkg (lockfileDir: string, virtualStoreDir: string, pkgDir: string): Promise<void> {
  const pathToRemove = path.join(virtualStoreDir, pkgDir)
  removalLogger.debug(pathToRemove)
  try {
    await rimraf(pathToRemove)
  } catch (err: any) { // eslint-disable-line
    logger.warn({
      error: err,
      message: `Failed to remove "${pathToRemove}"`,
      prefix: lockfileDir,
    })
  }
}

function mergeDependencies (projectSnapshot: ProjectSnapshot): { [depName: string]: string } {
  return mergeAll(
    DEPENDENCIES_FIELDS.map((depType) => projectSnapshot[depType] ?? {})
  )
}

function getPkgsDepPaths (
  packages: PackageSnapshots,
  skipped: Set<string>
): Record<DepPath, string> {
  const acc: Record<DepPath, string> = {}
  for (const [depPath, pkg] of Object.entries(packages)) {
    if (skipped.has(depPath)) continue
    acc[depPath as DepPath] = packageIdFromSnapshot(depPath as DepPath, pkg)
  }
  return acc
}

function getPkgsDepPathsOwnedOnlyByImporters (
  importerIds: ProjectId[],
  lockfile: LockfileObject,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    resolvePeersFromWorkspaceRoot?: boolean
    skipped: Set<DepPath>
  }
): Record<string, string> {
  const filterOpts = {
    failOnMissingDependencies: false,
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped: opts.skipped,
  }
  const selected = filterLockfileByImporters(lockfile, importerIds, filterOpts)
  const other = filterLockfileByImporters(lockfile,
    difference(Object.keys(lockfile.importers) as ProjectId[], importerIds),
    filterOpts)
  const packagesOfSelectedOnly = pickAll(
    difference(Object.keys(selected.packages!), Object.keys(other.packages!)),
    selected.packages!
  ) as PackageSnapshots
  return getPkgsDepPaths(packagesOfSelectedOnly, opts.skipped)
}

function getPubliclyHoistedDependencies (hoistedDependencies: HoistedDependencies): Set<string> {
  const publiclyHoistedDeps = new Set<string>()
  for (const hoistedAliases of Object.values(hoistedDependencies)) {
    for (const [alias, hoistType] of Object.entries(hoistedAliases)) {
      if (hoistType === 'public') {
        publiclyHoistedDeps.add(alias)
      }
    }
  }
  return publiclyHoistedDeps
}
