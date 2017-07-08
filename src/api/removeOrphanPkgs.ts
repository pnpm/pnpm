import rimraf = require('rimraf-then')
import path = require('path')
import {
  shortIdToFullId,
} from '../fs/shrinkwrap'
import {Shrinkwrap} from 'pnpm-shrinkwrap'
import {Store, save as saveStore, PackageSpec} from 'package-store'
import R = require('ramda')
import removeTopDependency from '../removeTopDependency'
import logger from 'pnpm-logger'
import {dependenciesTypes} from '../getSaveType'

export default async function removeOrphanPkgs (
  opts: {
    oldShrinkwrap: Shrinkwrap,
    newShrinkwrap: Shrinkwrap,
    prefix: string,
    store: string,
    storeIndex: Store,
  }
): Promise<string[]> {
  const oldPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.oldShrinkwrap[depType], dependenciesTypes)))
  const newPkgs = R.toPairs(R.mergeAll(R.map(depType => opts.newShrinkwrap[depType], dependenciesTypes)))

  const removedTopDeps: [string, string][] = R.difference(oldPkgs, newPkgs) as [string, string][]

  const rootModules = path.join(opts.prefix, 'node_modules')
  await Promise.all(removedTopDeps.map(depName => removeTopDependency(depName[0], rootModules)))

  const oldPkgIds = R.keys(opts.oldShrinkwrap.packages).map(shortId => shortIdToFullId(shortId, opts.oldShrinkwrap.registry))
  const newPkgIds = R.keys(opts.newShrinkwrap.packages).map(shortId => shortIdToFullId(shortId, opts.newShrinkwrap.registry))

  const notDependents = R.difference(oldPkgIds, newPkgIds)

  if (notDependents.length) {
    logger.info(`Removing ${notDependents.length} orphan packages from node_modules`);

    await Promise.all(notDependents.map(async notDependent => {
      if (opts.storeIndex[notDependent]) {
        opts.storeIndex[notDependent].splice(opts.storeIndex[notDependent].indexOf(opts.prefix), 1)
        if (!opts.storeIndex[notDependent].length) {
          delete opts.storeIndex[notDependent]
          await rimraf(path.join(opts.store, notDependent))
        }
      }
      await rimraf(path.join(rootModules, `.${notDependent}`))
    }))
  }

  const newDependents = R.difference(newPkgIds, oldPkgIds)

  newDependents.forEach(newDependent => {
    opts.storeIndex[newDependent] = opts.storeIndex[newDependent] || []
    if (opts.storeIndex[newDependent].indexOf(opts.prefix) === -1) {
      opts.storeIndex[newDependent].push(opts.prefix)
    }
  })

  await saveStore(opts.store, opts.storeIndex)

  return notDependents
}
