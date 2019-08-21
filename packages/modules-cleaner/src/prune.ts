import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import filterLockfile, { filterLockfileByImporters } from '@pnpm/filter-lockfile'
import {
  Lockfile,
  LockfileImporter,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import { packageIdFromSnapshot } from '@pnpm/lockfile-utils'
import logger from '@pnpm/logger'
import readModulesDir from '@pnpm/read-modules-dir'
import { StoreController } from '@pnpm/store-controller-types'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  Registries,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import vacuumCB = require('fs-vacuum')
import path = require('path')
import R = require('ramda')
import { promisify } from 'util'
import removeDirectDependency from './removeDirectDependency'

const vacuum = promisify(vacuumCB)

export default async function prune (
  importers: Array<{
    bin: string,
    hoistedAliases: {[depPath: string]: string[]},
    id: string,
    modulesDir: string,
    prefix: string,
    pruneDirectDependencies?: boolean,
    removePackages?: string[],
    shamefullyFlatten: boolean | string,
  }>,
  opts: {
    dryRun?: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    wantedLockfile: Lockfile,
    currentLockfile: Lockfile,
    pruneStore?: boolean,
    registries: Registries,
    skipped: Set<string>,
    virtualStoreDir: string,
    lockfileDirectory: string,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  const wantedLockfile = filterLockfile(opts.wantedLockfile, {
    include: opts.include,
    registries: opts.registries,
    skipped: opts.skipped,
  })
  await Promise.all(importers.map(async ({ bin, id, modulesDir, prefix, pruneDirectDependencies, removePackages }) => {
    const currentImporter = opts.currentLockfile.importers[id] || {} as LockfileImporter
    const currentPkgs = R.toPairs(mergeDependencies(currentImporter))
    const wantedPkgs = R.toPairs(mergeDependencies(wantedLockfile.importers[id]))

    const allCurrentPackages = new Set(
      (pruneDirectDependencies || removePackages && removePackages.length)
        ? (await readModulesDir(modulesDir) || [])
        : [],
    )
    const depsToRemove = new Set([
      ...(removePackages || []).filter((removePackage) => allCurrentPackages.has(removePackage)),
      ...R.difference(currentPkgs, wantedPkgs).map(([depName]) => depName),
    ])
    if (pruneDirectDependencies) {
      if (allCurrentPackages.size > 0) {
        const newPkgsSet = new Set(wantedPkgs.map(([depName]) => depName))
        for (const currentPackage of Array.from(allCurrentPackages)) {
          if (!newPkgsSet.has(currentPackage)) {
            depsToRemove.add(currentPackage)
          }
        }
      }
    }

    return Promise.all(Array.from(depsToRemove).map((depName) => {
      return removeDirectDependency({
        dependenciesField: currentImporter.devDependencies && currentImporter.devDependencies[depName] && 'devDependencies' ||
          currentImporter.optionalDependencies && currentImporter.optionalDependencies[depName] && 'optionalDependencies' ||
          currentImporter.dependencies && currentImporter.dependencies[depName] && 'dependencies' ||
          undefined,
        name: depName,
      }, {
        bin,
        dryRun: opts.dryRun,
        modulesDir,
        prefix,
      })
    }))
  }))

  const selectedImporterIds = importers.map((importer) => importer.id).sort()
  // In case installation is done on a subset of importers,
  // we may only prune dependencies that are used only by that subset of importers.
  // Otherwise, we would break the node_modules.
  const currentPkgIdsByDepPaths = R.equals(selectedImporterIds, Object.keys(opts.currentLockfile.importers))
    ? getPkgsDepPaths(opts.registries, opts.currentLockfile.packages || {})
    : getPkgsDepPathsOwnedOnlyByImporters(selectedImporterIds, opts.registries, opts.currentLockfile, opts.include, opts.skipped)
  const wantedPkgIdsByDepPaths = getPkgsDepPaths(opts.registries, wantedLockfile.packages || {})

  const oldDepPaths = Object.keys(currentPkgIdsByDepPaths)
  const newDepPaths = Object.keys(wantedPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, currentPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.lockfileDirectory,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {
      if (opts.currentLockfile.packages) {
        await Promise.all(importers.filter((importer) => importer.shamefullyFlatten).map((importer) => {
          const { bin, hoistedAliases, modulesDir, prefix } = importer
          return Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
            if (hoistedAliases[orphanDepPath]) {
              await Promise.all(hoistedAliases[orphanDepPath].map((alias) => {
                return removeDirectDependency({
                  name: alias,
                }, {
                  bin,
                  modulesDir,
                  muteLogs: true,
                  prefix,
                })
              }))
            }
            delete hoistedAliases[orphanDepPath]
          }))
        }))
      }

      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        const pathToRemove = path.join(opts.virtualStoreDir, `.${orphanDepPath}`, 'node_modules')
        removalLogger.debug(pathToRemove)
        try {
          await vacuum(pathToRemove, {
            base: opts.virtualStoreDir,
            purge: true,
          })
        } catch (err) {
          logger.warn({
            error: err,
            message: `Failed to remove "${pathToRemove}"`,
            prefix: opts.lockfileDirectory,
          })
        }
      }))
    }

    const addedDepPaths = R.difference(newDepPaths, oldDepPaths)
    const addedPkgIds = new Set(R.props<string, string>(addedDepPaths, wantedPkgIdsByDepPaths))

    await opts.storeController.updateConnections(path.dirname(opts.virtualStoreDir), {
      addDependencies: Array.from(addedPkgIds),
      prune: opts.pruneStore || false,
      removeDependencies: Array.from(orphanPkgIds),
    })

    await opts.storeController.saveState()
  }

  return new Set(orphanDepPaths)
}

function mergeDependencies (lockfileImporter: LockfileImporter): { [depName: string]: string } {
  return R.mergeAll(
    DEPENDENCIES_FIELDS.map((depType) => lockfileImporter[depType] || {}),
  )
}

function getPkgsDepPaths (
  registries: Registries,
  packages: PackageSnapshots,
): {[depPath: string]: string} {
  const pkgIdsByDepPath = {}
  for (const relDepPath of Object.keys(packages)) {
    const depPath = dp.resolve(registries, relDepPath)
    pkgIdsByDepPath[depPath] = packageIdFromSnapshot(relDepPath, packages[relDepPath], registries)
  }
  return pkgIdsByDepPath
}

function getPkgsDepPathsOwnedOnlyByImporters (
  importerIds: string[],
  registries: Registries,
  lockfile: Lockfile,
  include: { [dependenciesField in DependenciesField]: boolean },
  skipped: Set<string>,
) {
  const selected = filterLockfileByImporters(lockfile,
    importerIds,
    {
      failOnMissingDependencies: false,
      include,
      registries,
      skipped,
    })
  const other = filterLockfileByImporters(lockfile,
    R.difference(Object.keys(lockfile.importers), importerIds),
    {
      failOnMissingDependencies: false,
      include,
      registries,
      skipped,
    })
  const packagesOfSelectedOnly = R.pickAll(R.difference(Object.keys(selected.packages!), Object.keys(other.packages!)), selected.packages!) as PackageSnapshots
  return getPkgsDepPaths(registries, packagesOfSelectedOnly)
}
