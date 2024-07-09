import { promises as fs } from 'fs'
import path from 'path'
import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { filterLockfile, filterLockfileByImporters } from '@pnpm/filter-lockfile'
import {
  type Lockfile,
  type PackageSnapshots,
  type ProjectSnapshot,
} from '@pnpm/lockfile-types'
import { packageIdFromSnapshot } from '@pnpm/lockfile-utils'
import { logger } from '@pnpm/logger'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type DepPath,
  type DependenciesField,
  DEPENDENCIES_FIELDS,
  type HoistedDependencies,
  type ProjectId,
  type ProjectRootDir,
} from '@pnpm/types'
import { depPathToFilename } from '@pnpm/dependency-path'
import rimraf from '@zkochan/rimraf'
import difference from 'ramda/src/difference'
import equals from 'ramda/src/equals'
import mergeAll from 'ramda/src/mergeAll'
import pickAll from 'ramda/src/pickAll'
import { removeDirectDependency, removeIfEmpty } from './removeDirectDependency'

export async function prune (
  importers: Array<{
    binsDir: string
    id: ProjectId
    modulesDir: string
    pruneDirectDependencies?: boolean
    removePackages?: string[]
    rootDir: ProjectRootDir
  }>,
  opts: {
    dedupeDirectDeps?: boolean
    dryRun?: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    hoistedDependencies: HoistedDependencies
    hoistedModulesDir?: string
    publicHoistedModulesDir?: string
    wantedLockfile: Lockfile
    currentLockfile: Lockfile
    pruneStore?: boolean
    pruneVirtualStore?: boolean
    skipped: Set<DepPath>
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    lockfileDir: string
    storeController: StoreController
  }
): Promise<Set<string>> {
  const wantedLockfile = filterLockfile(opts.wantedLockfile, {
    include: opts.include,
    skipped: opts.skipped,
  })
  const rootImporter = wantedLockfile.importers['.' as ProjectId] ?? {} as ProjectSnapshot
  const wantedRootPkgs = mergeDependencies(rootImporter)
  await Promise.all(importers.map(async ({ binsDir, id, modulesDir, pruneDirectDependencies, removePackages, rootDir }) => {
    const currentImporter = opts.currentLockfile.importers[id] || {} as ProjectSnapshot
    const currentPkgs = Object.entries(mergeDependencies(currentImporter))
    const wantedPkgs = mergeDependencies(wantedLockfile.importers[id])

    const allCurrentPackages = new Set(
      (pruneDirectDependencies === true || removePackages?.length)
        ? (await readModulesDir(modulesDir) ?? [])
        : []
    )
    const depsToRemove = new Set(
      (removePackages ?? []).filter((removePackage) => allCurrentPackages.has(removePackage))
    )
    currentPkgs.forEach(([depName, depVersion]) => {
      if (
        !wantedPkgs[depName] ||
        wantedPkgs[depName] !== depVersion ||
        opts.dedupeDirectDeps && id !== '.' && wantedPkgs[depName] === wantedRootPkgs[depName]
      ) {
        depsToRemove.add(depName)
      }
    })
    if (pruneDirectDependencies) {
      const publiclyHoistedDeps = getPubliclyHoistedDependencies(opts.hoistedDependencies)
      if (allCurrentPackages.size > 0) {
        for (const currentPackage of allCurrentPackages) {
          if (!wantedPkgs[currentPackage] && !publiclyHoistedDeps.has(currentPackage)) {
            depsToRemove.add(currentPackage)
          }
        }
      }
    }

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
        dryRun: opts.dryRun,
        modulesDir,
        rootDir,
      })
    }))
    await Promise.all(Array.from(removedFromScopes).map((scope) => removeIfEmpty(path.join(modulesDir, scope))))
    try {
      await removeIfEmpty(modulesDir)
    } catch {
      // On some server setups we might not have permission to remove the node_modules directory.
      // That's fine, just proceed.
    }
  }))

  const selectedImporterIds = importers.map((importer) => importer.id).sort()
  // In case installation is done on a subset of importers,
  // we may only prune dependencies that are used only by that subset of importers.
  // Otherwise, we would break the node_modules.
  const currentPkgIdsByDepPaths = equals(selectedImporterIds, Object.keys(opts.wantedLockfile.importers))
    ? getPkgsDepPaths(opts.currentLockfile.packages ?? {}, opts.skipped)
    : getPkgsDepPathsOwnedOnlyByImporters(selectedImporterIds, opts.currentLockfile, opts.include, opts.skipped)
  const wantedPkgIdsByDepPaths = getPkgsDepPaths(wantedLockfile.packages ?? {}, opts.skipped)

  const orphanDepPaths = (Object.keys(currentPkgIdsByDepPaths) as DepPath[]).filter((path: DepPath) => !wantedPkgIdsByDepPaths[path])
  const orphanPkgIds = new Set(orphanDepPaths.map(path => currentPkgIdsByDepPaths[path]))

  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (
      (orphanDepPaths.length > 0) &&
      (opts.currentLockfile.packages != null) &&
      (opts.hoistedModulesDir != null || opts.publicHoistedModulesDir != null)
    ) {
      const prefix = path.join(opts.virtualStoreDir, '../..')
      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        if (opts.hoistedDependencies[orphanDepPath]) {
          await Promise.all(Object.entries(opts.hoistedDependencies[orphanDepPath]).map(([alias, hoistType]) => {
            const modulesDir = hoistType === 'public'
              ? opts.publicHoistedModulesDir!
              : opts.hoistedModulesDir!
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
        }
        delete opts.hoistedDependencies[orphanDepPath]
      }))
    }

    if (opts.pruneVirtualStore !== false) {
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
  }

  return new Set(orphanDepPaths)
}

function getScopeFromPackageName (pkgName: string): string | undefined {
  if (pkgName[0] === '@') {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }
  return undefined
}

async function readVirtualStoreDir (virtualStoreDir: string, lockfileDir: string): Promise<string[]> {
  try {
    return await fs.readdir(virtualStoreDir)
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
  return Object.entries(packages).reduce((acc, [depPath, pkg]) => {
    if (skipped.has(depPath)) return acc
    acc[depPath as DepPath] = packageIdFromSnapshot(depPath as DepPath, pkg)
    return acc
  }, {} as Record<DepPath, string>)
}

function getPkgsDepPathsOwnedOnlyByImporters (
  importerIds: ProjectId[],
  lockfile: Lockfile,
  include: { [dependenciesField in DependenciesField]: boolean },
  skipped: Set<DepPath>
): Record<string, string> {
  const selected = filterLockfileByImporters(lockfile,
    importerIds,
    {
      failOnMissingDependencies: false,
      include,
      skipped,
    })
  const other = filterLockfileByImporters(lockfile,
    difference(Object.keys(lockfile.importers) as ProjectId[], importerIds),
    {
      failOnMissingDependencies: false,
      include,
      skipped,
    })
  const packagesOfSelectedOnly = pickAll(
    difference(Object.keys(selected.packages!), Object.keys(other.packages!)),
    selected.packages!
  ) as PackageSnapshots
  return getPkgsDepPaths(packagesOfSelectedOnly, skipped)
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
