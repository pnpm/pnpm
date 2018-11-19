import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import readModulesDir from '@pnpm/read-modules-dir'
import {
  PackageSnapshots,
  Shrinkwrap,
  ShrinkwrapImporter,
} from '@pnpm/shrinkwrap-types'
import { StoreController } from '@pnpm/store-controller-types'
import { DEPENDENCIES_FIELDS, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import vacuumCB = require('fs-vacuum')
import path = require('path')
import R = require('ramda')
import promisify = require('util.promisify')
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
    newShrinkwrap: Shrinkwrap,
    oldShrinkwrap: Shrinkwrap,
    pruneStore?: boolean,
    registries: Registries,
    virtualStoreDir: string,
    shrinkwrapDirectory: string,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  await Promise.all(opts.importers.map(async (importer) => {
    const oldImporterShr = opts.oldShrinkwrap.importers[importer.id] || {} as ShrinkwrapImporter
    const oldPkgs = R.toPairs(mergeDependencies(oldImporterShr))
    const newPkgs = R.toPairs(mergeDependencies(opts.newShrinkwrap.importers[importer.id]))

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
        dependenciesField: oldImporterShr.devDependencies && oldImporterShr.devDependencies[depName] && 'devDependencies' ||
          oldImporterShr.optionalDependencies && oldImporterShr.optionalDependencies[depName] && 'optionalDependencies' ||
          oldImporterShr.dependencies && oldImporterShr.dependencies[depName] && 'dependencies' ||
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

  const oldPkgIdsByDepPaths = getPkgsDepPaths(opts.registries.default, opts.oldShrinkwrap.packages || {})
  const newPkgIdsByDepPaths = getPkgsDepPaths(opts.registries.default, opts.newShrinkwrap.packages || {})

  const oldDepPaths = Object.keys(oldPkgIdsByDepPaths)
  const newDepPaths = Object.keys(newPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, oldPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.shrinkwrapDirectory,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {
      if (opts.oldShrinkwrap.packages) {
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
            prefix: opts.shrinkwrapDirectory,
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

function mergeDependencies (shrImporter: ShrinkwrapImporter): { [depName: string]: string } {
  return R.mergeAll(
    DEPENDENCIES_FIELDS.map((depType) => shrImporter[depType]),
  )
}

function getPkgsDepPaths (
  registry: string,
  packages: PackageSnapshots,
): {[depPath: string]: string} {
  const pkgIdsByDepPath = {}
  for (const relDepPath of Object.keys(packages)) {
    const depPath = dp.resolve(registry, relDepPath)
    pkgIdsByDepPath[depPath] = packages[relDepPath].id
      ? packages[relDepPath].id
      : depPath
  }
  return pkgIdsByDepPath
}
