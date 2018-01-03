import rimraf = require('rimraf-then')
import path = require('path')
import * as dp from 'dependency-path'
import {Shrinkwrap, ResolvedPackages} from 'pnpm-shrinkwrap'
import {StoreController} from 'package-store'
import R = require('ramda')
import removeTopDependency from '../removeTopDependency'
import logger from '@pnpm/logger'
import {dependenciesTypes} from '../getSaveType'
import {statsLogger} from '../loggers'

export default async function removeOrphanPkgs (
  opts: {
    dryRun?: boolean,
    oldShrinkwrap: Shrinkwrap,
    newShrinkwrap: Shrinkwrap,
    bin: string,
    prefix: string,
    storeController: StoreController,
    pruneStore?: boolean,
  }
): Promise<Set<string>> {
  const oldPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.oldShrinkwrap[depType], dependenciesTypes)))
  const newPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.newShrinkwrap[depType], dependenciesTypes)))

  const removedTopDeps: [string, string][] = R.difference(oldPkgs, newPkgs) as [string, string][]

  const rootModules = path.join(opts.prefix, 'node_modules')
  await Promise.all(removedTopDeps.map(depName => {
    return removeTopDependency({
      name: depName[0],
      dev: Boolean(opts.oldShrinkwrap.devDependencies && opts.oldShrinkwrap.devDependencies[depName[0]]),
      optional: Boolean(opts.oldShrinkwrap.optionalDependencies && opts.oldShrinkwrap.optionalDependencies[depName[0]]),
    }, {
      dryRun: opts.dryRun,
      modules: rootModules,
      bin: opts.bin,
    })
  }))

  const oldPkgIds = getPackageIds(opts.oldShrinkwrap.registry, opts.oldShrinkwrap.packages || {})
  const newPkgIds = getPackageIds(opts.newShrinkwrap.registry, opts.newShrinkwrap.packages || {})

  const notDependents = R.difference(oldPkgIds, newPkgIds)

  statsLogger.debug({removed: notDependents.length})

  if (!opts.dryRun) {
    if (notDependents.length) {
      await Promise.all(notDependents.map(async notDependent => {
        await rimraf(path.join(rootModules, `.${notDependent}`))
      }))
    }

    const newDependents = R.difference(newPkgIds, oldPkgIds)

    await opts.storeController.updateConnections(opts.prefix, {
      prune: opts.pruneStore || false,
      removeDependencies: notDependents,
      addDependencies: newDependents,
    })

    await opts.storeController.saveState()
  }

  return new Set(notDependents)
}

function getPackageIds (
  registry: string,
  packages: ResolvedPackages
): string[] {
  return R.uniq(
    R.keys(packages)
      .map(depPath => {
        if (packages[depPath].id) {
          return packages[depPath].id
        }
        return dp.resolve(registry, depPath)
      })
  ) as string[]
}
