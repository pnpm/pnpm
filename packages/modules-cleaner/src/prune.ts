import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { filterLockfileByImporters } from '@pnpm/filter-lockfile'
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
  opts: {
    dryRun?: boolean,
    importers: Array<{
      bin: string,
      hoistedAliases: {[depPath: string]: string[]},
      id: string,
      modulesDir: string,
      prefix: string,
      pruneDirectDependencies?: boolean,
      removePackages?: string[],
      shamefullyFlatten: boolean,
    }>,
    include: { [dependenciesField in DependenciesField]: boolean },
    newLockfile: Lockfile,
    oldLockfile: Lockfile,
    pruneStore?: boolean,
    registries: Registries,
    skipped: Set<string>,
    virtualStoreDir: string,
    lockfileDirectory: string,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  await Promise.all(opts.importers.map(async (importer) => {
    const oldLockfileImporter = opts.oldLockfile.importers[importer.id] || {} as LockfileImporter
    const oldPkgs = R.toPairs(mergeDependencies(oldLockfileImporter))
    const newPkgs = R.toPairs(mergeDependencies(opts.newLockfile.importers[importer.id]))

    const allCurrentPackages = new Set(
      (importer.pruneDirectDependencies || importer.removePackages && importer.removePackages.length)
        ? (await readModulesDir(importer.modulesDir) || [])
        : [],
    )
    const depsToRemove = new Set([
      ...(importer.removePackages || []).filter((removePackage) => allCurrentPackages.has(removePackage)),
      ...R.difference(oldPkgs, newPkgs).map(([depName]) => depName),
    ])
    if (importer.pruneDirectDependencies) {
      if (allCurrentPackages.size > 0) {
        const newPkgsSet = new Set(newPkgs.map(([depName]) => depName))
        for (const currentPackage of Array.from(allCurrentPackages)) {
          if (!newPkgsSet.has(currentPackage)) {
            depsToRemove.add(currentPackage)
          }
        }
      }
    }

    const { bin, modulesDir, prefix } = importer

    return Promise.all(Array.from(depsToRemove).map((depName) => {
      return removeDirectDependency({
        dependenciesField: oldLockfileImporter.devDependencies && oldLockfileImporter.devDependencies[depName] && 'devDependencies' ||
          oldLockfileImporter.optionalDependencies && oldLockfileImporter.optionalDependencies[depName] && 'optionalDependencies' ||
          oldLockfileImporter.dependencies && oldLockfileImporter.dependencies[depName] && 'dependencies' ||
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

  const selectedImporterIds = opts.importers.map((importer) => importer.id).sort()
  // In case installation is done on a subset of importers,
  // we may only prune dependencies that are used only by that subset of importers.
  // Otherwise, we would break the node_modules.
  const oldPkgIdsByDepPaths = R.equals(selectedImporterIds, Object.keys(opts.oldLockfile.importers))
    ? getPkgsDepPaths(opts.registries, opts.oldLockfile.packages || {})
    : getPkgsDepPathsOwnedOnlyByImporters(selectedImporterIds, opts.registries, opts.oldLockfile, opts.include, opts.skipped)
  const newPkgIdsByDepPaths = getPkgsDepPaths(opts.registries, opts.newLockfile.packages || {})

  const oldDepPaths = Object.keys(oldPkgIdsByDepPaths)
  const newDepPaths = Object.keys(newPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, oldPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.lockfileDirectory,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {
      if (opts.oldLockfile.packages) {
        await Promise.all(opts.importers.filter((importer) => importer.shamefullyFlatten).map((importer) => {
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
    const addedPkgIds = new Set(R.props<string, string>(addedDepPaths, newPkgIdsByDepPaths))

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
    DEPENDENCIES_FIELDS.map((depType) => lockfileImporter[depType]),
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
