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

  const oldDepPaths = R.keys(oldPkgIdsByDepPaths)
  const newDepPaths = R.keys(newPkgIdsByDepPaths)

  const notDependentDepPaths = R.difference(oldDepPaths, newDepPaths)
  const notDependentPkgIds = R.uniq(R.props(notDependentDepPaths, oldPkgIdsByDepPaths as any)) as any // tslint:disable-line

  statsLogger.debug({removed: notDependentPkgIds.length})

  if (!opts.dryRun) {
    if (notDependentDepPaths.length) {

      if (opts.shamefullyFlatten && opts.oldShrinkwrap.packages) {
        await Promise.all(notDependentDepPaths.map(async (notDependent) => {
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

      await Promise.all(notDependentDepPaths.map(async (notDependent) => {
        await rimraf(path.join(rootModules, `.${notDependent}`))
      }))
    }

    const newDependentDepPaths = R.difference(newDepPaths, oldDepPaths)
    const newDependentPkgIds = R.uniq(R.props(newDependentDepPaths, newPkgIdsByDepPaths as any)) as any // tslint:disable-line

    await opts.storeController.updateConnections(opts.prefix, {
      addDependencies: newDependentPkgIds,
      prune: opts.pruneStore || false,
      removeDependencies: notDependentPkgIds,
    })

    await opts.storeController.saveState()
  }

  return new Set(notDependentDepPaths)
}

function getPkgsDepPaths (
  registry: string,
  packages: ResolvedPackages,
): {[depPath: string]: string} {
  return R.keys(packages)
    .reduce((acc, relDepPath) => {
      const depPath = dp.resolve(registry, relDepPath)
      acc[depPath] = packages[relDepPath].id
        ? packages[relDepPath].id
        : depPath
      return acc
    }, {})
}
