import rimraf = require('rimraf-then')
import path = require('path')
import {
  shortIdToFullId,
} from '../fs/shrinkwrap'
import {Shrinkwrap} from 'pnpm-shrinkwrap'
import {read as readStore, save as saveStore} from '../fs/storeController'
import R = require('ramda')
import {PackageSpec} from '../resolve'
import removeTopDependency from '../removeTopDependency'
import logger from 'pnpm-logger'

export default async function removeOrphanPkgs (
  oldShr: Shrinkwrap,
  newShr: Shrinkwrap,
  root: string,
  storePath: string
): Promise<string[]> {
  const oldPkgNames = Object.keys(oldShr.specifiers)
  const newPkgNames = Object.keys(newShr.specifiers)

  const removedTopDeps = R.difference(oldPkgNames, newPkgNames)

  const rootModules = path.join(root, 'node_modules')
  await Promise.all(removedTopDeps.map(depName => removeTopDependency(depName, rootModules)))

  const oldPkgIds = R.keys(oldShr.packages).map(shortId => shortIdToFullId(shortId, oldShr.registry))
  const newPkgIds = R.keys(newShr.packages).map(shortId => shortIdToFullId(shortId, newShr.registry))

  const store = await readStore(storePath) || {}
  const notDependents = R.difference(oldPkgIds, newPkgIds)

  if (notDependents.length) {
    logger.info(`Removing ${notDependents.length} orphan packages from node_modules`);

    await Promise.all(notDependents.map(async notDependent => {
      if (store[notDependent]) {
        store[notDependent].splice(store[notDependent].indexOf(root), 1)
        if (!store[notDependent].length) {
          delete store[notDependent]
          await rimraf(path.join(storePath, notDependent))
        }
      }
      await rimraf(path.join(rootModules, `.${notDependent}`))
    }))
  }

  const newDependents = R.difference(newPkgIds, oldPkgIds)

  newDependents.forEach(newDependent => {
    store[newDependent] = store[newDependent] || []
    if (store[newDependent].indexOf(root) === -1) {
      store[newDependent].push(root)
    }
  })

  await saveStore(storePath, store)

  return notDependents
}
