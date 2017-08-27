import rimraf = require('rimraf-then')
import path = require('path')
import * as dp from 'dependency-path'
import {Shrinkwrap, ResolvedPackages} from 'pnpm-shrinkwrap'
import {Store, save as saveStore, PackageSpec} from 'package-store'
import R = require('ramda')
import removeTopDependency from '../removeTopDependency'
import logger from 'pnpm-logger'
import {dependenciesTypes} from '../getSaveType'

export default async function removeOrphanPkgs (
  opts: {
    oldShrinkwrap: Shrinkwrap,
    newShrinkwrap: Shrinkwrap,
    bin: string,
    prefix: string,
    store: string,
    storeIndex: Store,
    pruneStore?: boolean,
  }
): Promise<string[]> {
  const oldPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.oldShrinkwrap[depType], dependenciesTypes)))
  const newPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.newShrinkwrap[depType], dependenciesTypes)))

  const removedTopDeps: [string, string][] = R.difference(oldPkgs, newPkgs) as [string, string][]

  const rootModules = path.join(opts.prefix, 'node_modules')
  const waitq = []
  waitq.push(Promise.all(removedTopDeps.map(depName => {
    return removeTopDependency({
      name: depName[0],
      dev: Boolean(opts.oldShrinkwrap.devDependencies && opts.oldShrinkwrap.devDependencies[depName[0]]),
      optional: Boolean(opts.oldShrinkwrap.optionalDependencies && opts.oldShrinkwrap.optionalDependencies[depName[0]]),
    }, {
      modules: rootModules,
      bin: opts.bin,
    })
  })))

  const oldPkgIds = getPackageIds(opts.oldShrinkwrap.registry, opts.oldShrinkwrap.packages || {})
  const newPkgIds = getPackageIds(opts.newShrinkwrap.registry, opts.newShrinkwrap.packages || {})

  const notDependents = R.difference(oldPkgIds, newPkgIds)

  if (notDependents.length) {
    logger.info(`Removing ${notDependents.length} orphan packages from node_modules`);

    waitq.push(Promise.all(notDependents.map(async notDependent => {
      if (opts.storeIndex[notDependent]) {
        opts.storeIndex[notDependent].splice(opts.storeIndex[notDependent].indexOf(opts.prefix), 1)
        if (opts.pruneStore && !opts.storeIndex[notDependent].length) {
          delete opts.storeIndex[notDependent]
          await rimraf(path.join(opts.store, notDependent))
        }
      }
      await rimraf(path.join(rootModules, `.${notDependent}`))
    })))
  }

  const newDependents = R.difference(newPkgIds, oldPkgIds)

  newDependents.forEach(newDependent => {
    opts.storeIndex[newDependent] = opts.storeIndex[newDependent] || []
    if (opts.storeIndex[newDependent].indexOf(opts.prefix) === -1) {
      opts.storeIndex[newDependent].push(opts.prefix)
    }
  })

  waitq.push(saveStore(opts.store, opts.storeIndex))

  await Promise.all(waitq)

  return notDependents
}

function getPackageIds (
  registry: string,
  packages: ResolvedPackages
): string[] {
  return R.keys(packages)
    .map(depPath => {
      if (packages[depPath].id) {
        return packages[depPath].id
      }
      return dp.resolve(registry, depPath)
    }) as string[]
}
