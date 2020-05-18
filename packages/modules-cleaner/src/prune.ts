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
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import readModulesDir from '@pnpm/read-modules-dir'
import { StoreController } from '@pnpm/store-controller-types'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  Registries,
} from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import path = require('path')
import R = require('ramda')
import removeDirectDependency from './removeDirectDependency'

export default async function prune (
  importers: Array<{
    binsDir: string,
    id: string,
    modulesDir: string,
    pruneDirectDependencies?: boolean,
    removePackages?: string[],
    rootDir: string,
  }>,
  opts: {
    dryRun?: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    hoistedAliases: {[depPath: string]: string[]},
    hoistedModulesDir?: string,
    wantedLockfile: Lockfile,
    currentLockfile: Lockfile,
    pruneStore?: boolean,
    registries: Registries,
    skipped: Set<string>,
    virtualStoreDir: string,
    lockfileDir: string,
    storeController: StoreController,
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
      (pruneDirectDependencies || removePackages?.length)
        ? (await readModulesDir(modulesDir) || [])
        : []
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
        dependenciesField: currentImporter.devDependencies?.[depName] && 'devDependencies' ||
          currentImporter.optionalDependencies?.[depName] && 'optionalDependencies' ||
          currentImporter.dependencies?.[depName] && 'dependencies' ||
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
  const wantedPkgIdsByDepPaths = getPkgsDepPaths(opts.registries, wantedLockfile.packages || {}, opts.skipped)

  const oldDepPaths = Object.keys(currentPkgIdsByDepPaths)
  const newDepPaths = Object.keys(wantedPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, currentPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {
      if (opts.currentLockfile.packages && opts.hoistedModulesDir) {
        const modulesDir = opts.hoistedModulesDir
        const binsDir = path.join(opts.hoistedModulesDir, '.bin')
        const prefix = path.join(opts.virtualStoreDir, '../..')
        await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
          if (opts.hoistedAliases[orphanDepPath]) {
            await Promise.all(opts.hoistedAliases[orphanDepPath].map((alias) => {
              return removeDirectDependency({
                name: alias,
              }, {
                binsDir,
                modulesDir,
                muteLogs: true,
                rootDir: prefix,
              })
            }))
          }
          delete opts.hoistedAliases[orphanDepPath]
        }))
      }

      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        const pathToRemove = path.join(opts.virtualStoreDir, pkgIdToFilename(orphanDepPath, opts.lockfileDir))
        removalLogger.debug(pathToRemove)
        try {
          await rimraf(pathToRemove)
        } catch (err) {
          logger.warn({
            error: err,
            message: `Failed to remove "${pathToRemove}"`,
            prefix: opts.lockfileDir,
          })
        }
      }))
    }
  }

  return new Set(orphanDepPaths)
}

function mergeDependencies (projectSnapshot: ProjectSnapshot): { [depName: string]: string } {
  return R.mergeAll(
    DEPENDENCIES_FIELDS.map((depType) => projectSnapshot[depType] || {})
  )
}

function getPkgsDepPaths (
  registries: Registries,
  packages: PackageSnapshots,
  skipped: Set<string>
): {[relDepPath: string]: string} {
  const pkgIdsByDepPath = {}
  for (const relDepPath of Object.keys(packages)) {
    if (skipped.has(relDepPath)) continue
    pkgIdsByDepPath[relDepPath] = packageIdFromSnapshot(relDepPath, packages[relDepPath], registries)
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
  const packagesOfSelectedOnly = R.pickAll(R.difference(Object.keys(selected.packages!), Object.keys(other.packages!)), selected.packages!) as PackageSnapshots
  return getPkgsDepPaths(registries, packagesOfSelectedOnly, skipped)
}
