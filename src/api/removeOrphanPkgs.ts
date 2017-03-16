import rimraf = require('rimraf-then')
import path = require('path')
import {
  Shrinkwrap,
  shortIdToFullId,
} from '../fs/shrinkwrap'
import {read as readStore, save as saveStore} from '../fs/storeController'
import binify from '../binify'
import safeReadPkg from '../fs/safeReadPkg'
import R = require('ramda')
import npa = require('npm-package-arg')
import {PackageSpec} from '../resolve'

export default async function removeOrphanPkgs (
  oldShr: Shrinkwrap,
  newShr: Shrinkwrap,
  root: string,
  storePath: string
) {
  const oldPkgNames = Object.keys(oldShr.dependencies).map(npa).map((spec: PackageSpec) => spec.name)
  const newPkgNames = Object.keys(newShr.dependencies).map(npa).map((spec: PackageSpec) => spec.name)

  const removedTopDeps = R.difference(oldPkgNames, newPkgNames)

  await Promise.all(removedTopDeps.map(depName => Promise.all([
    rimraf(path.join(root, 'node_modules', depName)),
    removeBins(depName, root),
  ])))

  const oldPkgIds = Object.keys(oldShr.packages).map(shortId => shortIdToFullId(shortId, oldShr.registry))
  const newPkgIds = Object.keys(newShr.packages).map(shortId => shortIdToFullId(shortId, newShr.registry))

  const store = await readStore(storePath) || {}
  const notDependents = R.difference(oldPkgIds, newPkgIds)

  await Promise.all(Array.from(notDependents).map(async notDependent => {
    if (store[notDependent]) {
      store[notDependent].splice(store[notDependent].indexOf(root), 1)
      if (!store[notDependent].length) {
        delete store[notDependent]
        await rimraf(path.join(storePath, notDependent))
      }
    }
    await rimraf(path.join(root, 'node_modules', `.${notDependent}`))
  }))

  const newDependents = R.difference(newPkgIds, oldPkgIds)

  newDependents.forEach(newDependent => {
    store[newDependent] = store[newDependent] || []
    if (store[newDependent].indexOf(root) === -1) {
      store[newDependent].push(root)
    }
  })

  await saveStore(storePath, store)
}

async function removeBins (uninstalledPkg: string, root: string) {
  const uninstalledPkgPath = path.join(root, 'node_modules', uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkg(uninstalledPkgPath)
  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)
  return Promise.all(
    cmds.map(cmd => rimraf(path.join(root, 'node_modules', '.bin', cmd.name)))
  )
}
