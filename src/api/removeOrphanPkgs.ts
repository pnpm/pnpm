import * as dp from 'dependency-path'
import vacuumCB = require('fs-vacuum')
import {StoreController} from 'package-store'
import path = require('path')
import {ResolvedPackages, Shrinkwrap} from 'pnpm-shrinkwrap'
import R = require('ramda')
import promisify = require('util.promisify')
import {dependenciesTypes} from '../getSaveType'
import {statsLogger} from '../loggers'
import removeTopDependency from '../removeTopDependency'

const vacuum = promisify(vacuumCB)

export default async function removeOrphanPkgs (
  opts: {
    bin: string,
    dryRun?: boolean,
    hoistedAliases: {[depPath: string]: string[]},
    newShrinkwrap: Shrinkwrap,
    oldShrinkwrap: Shrinkwrap,
    prefix: string,
    pruneStore?: boolean,
    shamefullyFlatten: boolean,
    storeController: StoreController,
  },
): Promise<Set<string>> {
  const oldPkgs = R.toPairs(R.mergeAll(R.map((depType) => opts.oldShrinkwrap[depType], dependenciesTypes)))
  const newPkgs = R.toPairs(R.mergeAll(R.map((depType) => opts.newShrinkwrap[depType], dependenciesTypes)))

  const removedTopDeps: Array<[string, string]> = R.difference(oldPkgs, newPkgs) as Array<[string, string]>

  const rootModules = path.join(opts.prefix, 'node_modules')
  await Promise.all(removedTopDeps.map((depName) => {
    return removeTopDependency({
      dev: Boolean(opts.oldShrinkwrap.devDependencies && opts.oldShrinkwrap.devDependencies[depName[0]]),
      name: depName[0],
      optional: Boolean(opts.oldShrinkwrap.optionalDependencies && opts.oldShrinkwrap.optionalDependencies[depName[0]]),
    }, {
      bin: opts.bin,
      dryRun: opts.dryRun,
      modules: rootModules,
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
              })
            }))
          }
          delete opts.hoistedAliases[orphanDepPath]
        }))
      }

      await Promise.all(orphanDepPaths.map(async (orphanDepPath) => {
        await vacuum(path.join(rootModules, `.${orphanDepPath}`, 'node_modules'), {
           base: rootModules,
           purge: true,
        })
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
