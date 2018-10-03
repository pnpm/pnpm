import {
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
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
    importers: {
      [importerPath: string]: {
        bin: string,
        hoistedAliases: {[depPath: string]: string[]},
        importerModulesDir: string,
        shamefullyFlatten: boolean,
        prefix: string,
      },
    },
    newShrinkwrap: Shrinkwrap,
    oldShrinkwrap: Shrinkwrap,
    pruneStore?: boolean,
    virtualStoreDir: string,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  for (const importerPath of Object.keys(opts.importers)) {
    const oldImporterShr = opts.oldShrinkwrap.importers[importerPath] || {}
    const oldPkgs = R.toPairs(R.mergeAll(R.map((depType) => oldImporterShr[depType], DEPENDENCIES_FIELDS)))
    const newPkgs = R.toPairs(R.mergeAll(R.map((depType) => opts.newShrinkwrap.importers[importerPath][depType], DEPENDENCIES_FIELDS)))

    const removedTopDeps: Array<[string, string]> = R.difference(oldPkgs, newPkgs) as Array<[string, string]>

    const {bin, importerModulesDir, prefix} = opts.importers[importerPath]

    await Promise.all(removedTopDeps.map((depName) => {
      return removeDirectDependency({
        dev: Boolean(oldImporterShr.devDependencies && oldImporterShr.devDependencies[depName[0]]),
        name: depName[0],
        optional: Boolean(oldImporterShr.optionalDependencies && oldImporterShr.optionalDependencies[depName[0]]),
      }, {
        bin,
        dryRun: opts.dryRun,
        importerModulesDir,
        prefix,
      })
    }))
  }

  const oldPkgIdsByDepPaths = getPkgsDepPaths(opts.oldShrinkwrap.registry, opts.oldShrinkwrap.packages || {})
  const newPkgIdsByDepPaths = getPkgsDepPaths(opts.newShrinkwrap.registry, opts.newShrinkwrap.packages || {})

  const oldDepPaths = Object.keys(oldPkgIdsByDepPaths)
  const newDepPaths = Object.keys(newPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, oldPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.virtualStoreDir,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {
      if (opts.oldShrinkwrap.packages) {
        for (const importerPath of Object.keys(opts.importers)) {
          if (!opts.importers[importerPath].shamefullyFlatten) continue

          const { bin, hoistedAliases, importerModulesDir, prefix } = opts.importers[importerPath]
          await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
            if (hoistedAliases[orphanDepPath]) {
              await Promise.all(hoistedAliases[orphanDepPath].map(async (alias) => {
                await removeDirectDependency({
                  dev: false,
                  name: alias,
                  optional: false,
                }, {
                  bin,
                  importerModulesDir,
                  muteLogs: true,
                  prefix,
                })
              }))
            }
            delete hoistedAliases[orphanDepPath]
          }))
        }
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
            prefix: opts.virtualStoreDir,
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
