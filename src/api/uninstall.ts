import rimraf = require('rimraf-then')
import path = require('path')

import getContext, {PnpmContext} from './getContext'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import binify from '../binify'
import extendOptions from './extendOptions'
import requireJson from '../fs/requireJson'
import {PnpmOptions, StrictPnpmOptions, Package} from '../types'
import {StoreJson} from '../fs/storeJsonController'
import lock from './lock'
import {save as saveStoreJson} from '../fs/storeJsonController'

export default async function uninstallCmd (pkgsToUninstall: string[], maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (!ctx.pkg) {
    throw new Error('No package.json found - cannot uninstall')
  }

  const pkg = ctx.pkg
  return lock(ctx.store, () => uninstallInContext(pkgsToUninstall, pkg, ctx, opts))
}

export async function uninstallInContext (pkgsToUninstall: string[], pkg: Package, ctx: PnpmContext, opts: StrictPnpmOptions) {
  pkg.dependencies = pkg.dependencies || {}

  // this is OK. The store might not have records for the package
  // maybe it was cloned, `pnpm install` was not executed
  // and remove is done on a package with no dependencies installed
  ctx.storeJson.dependencies[ctx.root] = ctx.storeJson.dependencies[ctx.root] || {}

  const pkgIds = <string[]>pkgsToUninstall
    .map(dep => ctx.storeJson.dependencies[ctx.root][dep])
    .filter(pkgId => !!pkgId)
  const uninstalledPkgs = tryUninstall(pkgIds.slice(), ctx.storeJson, ctx.root)
  await Promise.all(
    uninstalledPkgs.map(uninstalledPkg => removeBins(uninstalledPkg, ctx.store, ctx.root))
  )
  if (ctx.storeJson.dependencies[ctx.root]) {
    pkgsToUninstall.forEach(dep => {
      delete ctx.storeJson.dependencies[ctx.root][dep]
    })
    if (!Object.keys(ctx.storeJson.dependencies[ctx.root]).length) {
      delete ctx.storeJson.dependencies[ctx.root]
    }
  }
  await Promise.all(uninstalledPkgs.map(pkgId => removePkgFromStore(pkgId, ctx.store)))

  saveStoreJson(ctx.store, ctx.storeJson)
  await Promise.all(pkgsToUninstall.map(dep => rimraf(path.join(ctx.root, 'node_modules', dep))))

  const saveType = getSaveType(opts)
  if (saveType) {
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    await removeDeps(pkgJsonPath, pkgsToUninstall, saveType)
  }
}

function canBeUninstalled (pkgId: string, storeJson: StoreJson, pkgPath: string) {
  return !storeJson.dependents[pkgId] || !storeJson.dependents[pkgId].length ||
    storeJson.dependents[pkgId].length === 1 && storeJson.dependents[pkgId].indexOf(pkgPath) !== -1
}

export function tryUninstall (pkgIds: string[], storeJson: StoreJson, pkgPath: string) {
  const uninstalledPkgs: string[] = []
  let numberOfUninstalls: number
  do {
    numberOfUninstalls = 0
    for (let i = 0; i < pkgIds.length; ) {
      if (canBeUninstalled(pkgIds[i], storeJson, pkgPath)) {
        const uninstalledPkg = pkgIds.splice(i, 1)[0]
        uninstalledPkgs.push(uninstalledPkg)
        const deps = storeJson.dependencies[uninstalledPkg] || {}
        const depIds = Object.keys(deps).map(depName => deps[depName])
        delete storeJson.dependencies[uninstalledPkg]
        delete storeJson.dependents[uninstalledPkg]
        depIds.forEach((dep: string) => removeDependency(dep, uninstalledPkg, storeJson))
        Array.prototype.push.apply(uninstalledPkgs, tryUninstall(depIds, storeJson, pkgPath))
        numberOfUninstalls++
        continue
      }
      i++
    }
  } while (numberOfUninstalls)
  return uninstalledPkgs
}

function removeDependency (dependentPkgName: string, uninstalledPkg: string, storeJson: StoreJson) {
  if (!storeJson.dependents[dependentPkgName]) return
  storeJson.dependents[dependentPkgName].splice(storeJson.dependents[dependentPkgName].indexOf(uninstalledPkg), 1)
  if (!storeJson.dependents[dependentPkgName].length) {
    delete storeJson.dependents[dependentPkgName]
  }
}

function removeBins (uninstalledPkg: string, store: string, root: string) {
  const uninstalledPkgJson = requireJson(path.join(store, uninstalledPkg, '_/package.json'))
  const bins = binify(uninstalledPkgJson)
  return Promise.all(
    Object.keys(bins).map(bin => rimraf(path.join(root, 'node_modules/.bin', bin)))
  )
}

export function removePkgFromStore (pkgId: string, store: string) {
  return rimraf(path.join(store, pkgId))
}
