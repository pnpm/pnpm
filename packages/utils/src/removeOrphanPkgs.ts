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
import removeTopDependency from './removeTopDependency'

const vacuum = promisify(vacuumCB)

export default async function removeOrphanPkgs (
  opts: {
    bin: string,
    dryRun?: boolean,
    hoistedAliases: {[depPath: string]: string[]},
    importerPath: string,
    newShrinkwrap: Shrinkwrap,
    oldShrinkwrap: Shrinkwrap,
    prefix: string,
    pruneStore?: boolean,
    shamefullyFlatten: boolean,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  const oldImporterShr = opts.oldShrinkwrap.importers[opts.importerPath] || {}
  const oldPkgs = R.toPairs(R.mergeAll(R.map((depType) => oldImporterShr[depType], DEPENDENCIES_FIELDS)))
  const newPkgs = R.toPairs(R.mergeAll(R.map((depType) => opts.newShrinkwrap.importers[opts.importerPath][depType], DEPENDENCIES_FIELDS)))

  const removedTopDeps: Array<[string, string]> = R.difference(oldPkgs, newPkgs) as Array<[string, string]>

  const rootModules = path.join(opts.prefix, 'node_modules')
  await Promise.all(removedTopDeps.map((depName) => {
    return removeTopDependency({
      dev: Boolean(oldImporterShr.devDependencies && oldImporterShr.devDependencies[depName[0]]),
      name: depName[0],
      optional: Boolean(oldImporterShr.optionalDependencies && oldImporterShr.optionalDependencies[depName[0]]),
    }, {
      bin: opts.bin,
      dryRun: opts.dryRun,
      modules: rootModules,
      prefix: opts.prefix,
    })
  }))

  const oldPkgIdsByDepPaths = getPkgsDepPaths(opts.oldShrinkwrap.registry, opts.oldShrinkwrap.packages || {})
  const newPkgIdsByDepPaths = getPkgsDepPaths(opts.newShrinkwrap.registry, opts.newShrinkwrap.packages || {})

  const oldDepPaths = Object.keys(oldPkgIdsByDepPaths)
  const newDepPaths = Object.keys(newPkgIdsByDepPaths)

  const orphanDepPaths = R.difference(oldDepPaths, newDepPaths)
  const orphanPkgIds = new Set(R.props<string, string>(orphanDepPaths, oldPkgIdsByDepPaths))

  statsLogger.debug({
    prefix: opts.prefix,
    removed: orphanPkgIds.size,
  })

  if (!opts.dryRun) {
    if (orphanDepPaths.length) {

      if (opts.shamefullyFlatten && opts.oldShrinkwrap.packages) {
        await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
          if (opts.hoistedAliases[orphanDepPath]) {
            await Promise.all(opts.hoistedAliases[orphanDepPath].map(async (alias) => {
              await removeTopDependency({
                dev: false,
                name: alias,
                optional: false,
              }, {
                bin: opts.bin,
                modules: rootModules,
                muteLogs: true,
                prefix: opts.prefix,
              })
            }))
          }
          delete opts.hoistedAliases[orphanDepPath]
        }))
      }

      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        const pathToRemove = path.join(rootModules, `.${orphanDepPath}`, 'node_modules')
        removalLogger.debug(pathToRemove)
        try {
          await vacuum(pathToRemove, {
            base: rootModules,
            purge: true,
          })
        } catch (err) {
          logger.warn({
            error: err,
            message: `Failed to remove "${pathToRemove}"`,
            prefix: opts.prefix,
          })
        }
      }))
    }

    const addedDepPaths = R.difference(newDepPaths, oldDepPaths)
    const addedPkgIds = new Set(R.props<string, string>(addedDepPaths, newPkgIdsByDepPaths))

    await opts.storeController.updateConnections(opts.prefix, {
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
