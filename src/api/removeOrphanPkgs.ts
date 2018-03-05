import * as dp from 'dependency-path'
import {StoreController} from 'package-store'
import path = require('path')
import {ResolvedPackages, Shrinkwrap} from 'pnpm-shrinkwrap'
import R = require('ramda')
import rimraf = require('rimraf-then')
import {dependenciesTypes} from '../getSaveType'
import {statsLogger} from '../loggers'
import removeTopDependency from '../removeTopDependency'

export default async function removeOrphanPkgs (
  opts: {
    bin: string,
    dryRun?: boolean,
    hoistedAliases: {[pkgId: string]: string[]},
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

  const oldPkgIds = getPackageIds(opts.oldShrinkwrap.registry, opts.oldShrinkwrap.packages || {})
  const newPkgIds = getPackageIds(opts.newShrinkwrap.registry, opts.newShrinkwrap.packages || {})

  const notDependents = R.difference(oldPkgIds, newPkgIds)

  statsLogger.debug({removed: notDependents.length})

  if (!opts.dryRun) {
    if (notDependents.length) {

      if (opts.shamefullyFlatten && opts.oldShrinkwrap.packages) {
        await Promise.all(notDependents.map(async (notDependent) => {
          if (opts.hoistedAliases[notDependent]) {
            await Promise.all(opts.hoistedAliases[notDependent].map(async (alias) => {
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
          delete opts.hoistedAliases[notDependent]
        }))
      }

      await Promise.all(notDependents.map(async (notDependent) => {
        await rimraf(path.join(rootModules, `.${notDependent}`))
      }))
    }

    const newDependents = R.difference(newPkgIds, oldPkgIds)

    await opts.storeController.updateConnections(opts.prefix, {
      addDependencies: newDependents,
      prune: opts.pruneStore || false,
      removeDependencies: notDependents,
    })

    await opts.storeController.saveState()
  }

  return new Set(notDependents)
}

function getPackageIds (
  registry: string,
  packages: ResolvedPackages,
): string[] {
  return R.uniq(
    R.keys(packages)
      .map((depPath) => {
        if (packages[depPath].id) {
          return packages[depPath].id
        }
        return dp.resolve(registry, depPath)
      }),
  ) as string[]
}
