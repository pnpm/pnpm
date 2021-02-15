import { promises as fs } from 'fs'
import path from 'path'
import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import filterLockfile, { filterLockfileByImporters } from '@pnpm/filter-lockfile'
import {
  Lockfile,
  PackageSnapshots,
  ProjectSnapshot,
} from '@pnpm/lockfile-types'
import { packageIdFromSnapshot } from '@pnpm/lockfile-utils'
import logger from '@pnpm/logger'
import readModulesDir from '@pnpm/read-modules-dir'
import { StoreController } from '@pnpm/store-controller-types'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  HoistedDependencies,
  Registries,
} from '@pnpm/types'
import { depPathToFilename } from 'dependency-path'
import rimraf from '@zkochan/rimraf'
import * as R from 'ramda'
import removeDirectDependency from './removeDirectDependency'

export default async function prune (
  importers: Array<{
    binsDir: string
    id: string
    modulesDir: string
    pruneDirectDependencies?: boolean
    removePackages?: string[]
    rootDir: string
  }>,
  opts: {
    dryRun?: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    hoistedDependencies: HoistedDependencies
    hoistedModulesDir?: string
    publicHoistedModulesDir?: string
    wantedLockfile: Lockfile
    currentLockfile: Lockfile
    pruneStore?: boolean
    pruneVirtualStore?: boolean
    registries: Registries
    skipped: Set<string>
    virtualStoreDir: string
    lockfileDir: string
    storeController: StoreController
  }
): Promise<Set<string>> {
  const wantedLockfile = filterLockfile(opts.wantedLockfile, {
    include: opts.include,
    skipped: opts.skipped,
  })
  await Promise.all(importers.map(async ({ binsDir, id, modulesDir, pruneDirectDependencies, removePackages, rootDir }) => {
    const currentImporter = opts.currentLockfile.importers[id] || {} as ProjectSnapshot
    const currentPkgs = R.toPairs(mergeDependencies(currentImporter))
    const wantedPkgs = R.toPairs(mergeDependencies(wantedLockfile.importers[id]))

    const allCurrentPackages = new Set(
      (pruneDirectDependencies === true || removePackages?.length)
        ? (await readModulesDir(modulesDir) ?? [])
        : []
    )
    const depsToRemove = new Set([
      ...(removePackages ?? []).filter((removePackage) => allCurrentPackages.has(removePackage)),
      ...R.difference(currentPkgs, wantedPkgs).map(([depName]) => depName),
    ])
    if (pruneDirectDependencies) {
      if (allCurrentPackages.size > 0) {
        const newPkgsSet = new Set<string>(wantedPkgs.map(([depName]) => depName))
        for (const currentPackage of Array.from(allCurrentPackages)) {
          if (!newPkgsSet.has(currentPackage)) {
            depsToRemove.add(currentPackage)
          }
        }
      }
    }

    return Promise.all(Array.from(depsToRemove).map(async (depName) => {
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
  }))

  const selectedImporterIds = importers.map((importer) => importer.id).sort()
  // In case installation is done on a subset of importers,
  // we may only prune dependencies that are used only by that subset of importers.
  // Otherwise, we would break the node_modules.
  const currentPkgIdsByDepPaths = R.equals(selectedImporterIds, Object.keys(opts.wantedLockfile.importers))
    ? getPkgsDepPaths(opts.registries, opts.currentLockfile.packages ?? {}, opts.skipped)
    : getPkgsDepPathsOwnedOnlyByImporters(selectedImporterIds, opts.registries, opts.currentLockfile, opts.include, opts.skipped)
  const wantedPkgIdsByDepPaths = getPkgsDepPaths(opts.registries, wantedLockfile.packages ?? {}, opts.skipped)

  const oldDepPaths = Object.keys(currentPkgIdsByDepPaths)
  const newDepPaths = Object.keys(wantedPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, currentPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (
      orphanDepPaths.length &&
      opts.currentLockfile.packages &&
      (opts.hoistedModulesDir != null || opts.publicHoistedModulesDir != null)
    ) {
      const prefix = path.join(opts.virtualStoreDir, '../..')
      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        if (opts.hoistedDependencies[orphanDepPath]) {
          await Promise.all(Object.entries(opts.hoistedDependencies[orphanDepPath]).map(([alias, hoistType]) => {
            const modulesDir = hoistType === 'public'
              ? opts.publicHoistedModulesDir! : opts.hoistedModulesDir!
            if (!modulesDir) return
            return removeDirectDependency({
              name: alias,
            }, {
              binsDir: path.join(modulesDir, '.bin'),
              modulesDir,
              muteLogs: true,
              rootDir: prefix,
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
          .map((orphanDepPath) => depPathToFilename(orphanDepPath, opts.lockfileDir))
          .map(async (orphanDepPath) => _tryRemovePkg(orphanDepPath))
      )
      const neededPkgs: Set<string> = new Set()
      for (const depPath of Object.keys(opts.wantedLockfile.packages ?? {})) {
        if (opts.skipped.has(depPath)) continue
        neededPkgs.add(depPathToFilename(depPath, opts.lockfileDir))
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

async function readVirtualStoreDir (virtualStoreDir: string, lockfileDir: string) {
  try {
    return await fs.readdir(virtualStoreDir)
  } catch (err) {
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

async function tryRemovePkg (lockfileDir: string, virtualStoreDir: string, pkgDir: string) {
  const pathToRemove = path.join(virtualStoreDir, pkgDir)
  removalLogger.debug(pathToRemove)
  try {
    await rimraf(pathToRemove)
  } catch (err) {
    logger.warn({
      error: err,
      message: `Failed to remove "${pathToRemove}"`,
      prefix: lockfileDir,
    })
  }
}

function mergeDependencies (projectSnapshot: ProjectSnapshot): { [depName: string]: string } {
  return R.mergeAll(
    DEPENDENCIES_FIELDS.map((depType) => projectSnapshot[depType] ?? {})
  )
}

function getPkgsDepPaths (
  registries: Registries,
  packages: PackageSnapshots,
  skipped: Set<string>
): {[depPath: string]: string} {
  const pkgIdsByDepPath = {}
  for (const depPath of Object.keys(packages)) {
    if (skipped.has(depPath)) continue
    pkgIdsByDepPath[depPath] = packageIdFromSnapshot(depPath, packages[depPath], registries)
  }
  return pkgIdsByDepPath
}

function getPkgsDepPathsOwnedOnlyByImporters (
  importerIds: string[],
  registries: Registries,
  lockfile: Lockfile,
  include: { [dependenciesField in DependenciesField]: boolean },
  skipped: Set<string>
) {
  const selected = filterLockfileByImporters(lockfile,
    importerIds,
    {
      failOnMissingDependencies: false,
      include,
      skipped,
    })
  const other = filterLockfileByImporters(lockfile,
    R.difference(Object.keys(lockfile.importers), importerIds),
    {
      failOnMissingDependencies: false,
      include,
      skipped,
    })
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-assertion
  const packagesOfSelectedOnly = R.pickAll(R.difference(Object.keys(selected.packages!), Object.keys(other.packages!)), selected.packages!) as PackageSnapshots
  return getPkgsDepPaths(registries, packagesOfSelectedOnly, skipped)
}
