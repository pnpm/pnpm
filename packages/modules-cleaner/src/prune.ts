import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import { DEPENDENCIES_FIELDS, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import vacuumCB = require('fs-vacuum')
import { StoreController } from 'package-store'
import path = require('path')
import { ResolvedPackages, Shrinkwrap } from 'pnpm-shrinkwrap'
import R = require('ramda')
import promisify = require('util.promisify')
import removeDirectDependency from './removeDirectDependency'

const vacuum = promisify(vacuumCB)

export default async function removeOrphanPkgs (
  opts: {
    dryRun?: boolean,
    importers: Array<{
      bin: string,
      hoistedAliases: {[depPath: string]: string[]},
      modulesDir: string,
      id: string,
      shamefullyFlatten: boolean,
      prefix: string,
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
  await Promise.all(opts.importers.map((importer) => {
    const oldImporterShr = opts.oldShrinkwrap.importers[importer.id] || {}
    const oldPkgs = R.toPairs(R.mergeAll(R.map((depType) => oldImporterShr[depType], DEPENDENCIES_FIELDS)))
    const newPkgs = R.toPairs(R.mergeAll(R.map((depType) => opts.newShrinkwrap.importers[importer.id][depType], DEPENDENCIES_FIELDS)))

    const removedTopDeps: Array<[string, string]> = R.difference(oldPkgs, newPkgs) as Array<[string, string]>

    const { bin, modulesDir, prefix } = importer

    return Promise.all(removedTopDeps.map((depName) => {
      return removeDirectDependency({
        dev: Boolean(oldImporterShr.devDependencies && oldImporterShr.devDependencies[depName[0]]),
        name: depName[0],
        optional: Boolean(oldImporterShr.optionalDependencies && oldImporterShr.optionalDependencies[depName[0]]),
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
                  dev: false,
                  name: alias,
                  optional: false,
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

function getPkgsDepPaths (
  registry: string,
  packages: ResolvedPackages,
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
