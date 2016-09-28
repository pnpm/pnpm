import cbRimraf = require('rimraf')
import path = require('path')

import initCmd, {CommandNamespace, PackageAndPath} from './initCmd'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import binify from '../binify'
import defaults from '../defaults'
import requireJson from '../fs/requireJson'
import {PnpmOptions, StrictPnpmOptions} from '../types'
import {StoreJson} from '../fs/storeJsonController'

export default async function uninstallCmd (pkgsToUninstall: string[], optsNullable: PnpmOptions) {
  const opts: StrictPnpmOptions = Object.assign({}, defaults, optsNullable)

  const cmd: CommandNamespace = await initCmd(opts)

  try {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot uninstall')
    }
    await uninstallInContext(pkgsToUninstall, cmd.pkg, cmd, opts)
    await cmd.unlock()
  } catch (err) {
    if (typeof cmd !== 'undefined' && cmd.unlock) await cmd.unlock()
    throw err
  }
}

export async function uninstallInContext (pkgsToUninstall: string[], pkg: PackageAndPath, cmd: CommandNamespace, opts: StrictPnpmOptions) {
  pkg.pkg.dependencies = pkg.pkg.dependencies || {}

  // this is OK. The store might not have records for the package
  // maybe it was cloned, `pnpm install` was not executed
  // and remove is done on a package with no dependencies installed
  cmd.ctx.storeJson.dependencies[pkg.path] = cmd.ctx.storeJson.dependencies[pkg.path] || {}

  const pkgIds = <string[]>pkgsToUninstall
    .map(dep => cmd.ctx.storeJson.dependencies[pkg.path][dep])
    .filter(pkgId => !!pkgId)
  const uninstalledPkgs = tryUninstall(pkgIds.slice(), cmd.ctx.storeJson, pkg.path)
  uninstalledPkgs.forEach(uninstalledPkg => removeBins(uninstalledPkg, cmd.ctx.store, cmd.ctx.root))
  if (cmd.ctx.storeJson.dependencies[pkg.path]) {
    pkgsToUninstall.forEach(dep => {
      delete cmd.ctx.storeJson.dependencies[pkg.path][dep]
    })
    if (!Object.keys(cmd.ctx.storeJson.dependencies[pkg.path]).length) {
      delete cmd.ctx.storeJson.dependencies[pkg.path]
    }
  }
  await Promise.all(uninstalledPkgs.map(pkgId => removePkgFromStore(pkgId, cmd.ctx.store)))

  cmd.storeJsonCtrl.save(cmd.ctx.storeJson)
  await Promise.all(pkgsToUninstall.map(dep => rimraf(path.join(cmd.ctx.root, 'node_modules', dep))))

  const saveType = getSaveType(opts)
  if (saveType) {
    await removeDeps(pkg.path, pkgsToUninstall, saveType)
  }
}

function canBeUninstalled (pkgId: string, storeJson: StoreJson, pkgPath: string) {
  return !storeJson.dependents[pkgId] || !storeJson.dependents[pkgId].length ||
    storeJson.dependents[pkgId].length === 1 && storeJson.dependents[pkgId].indexOf(pkgPath) !== -1
}

function tryUninstall (pkgIds: string[], storeJson: StoreJson, pkgPath: string) {
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
  Object.keys(bins).forEach(bin => cbRimraf.sync(path.join(root, 'node_modules/.bin', bin)))
}

function removePkgFromStore (pkgId: string, store: string) {
  return rimraf(path.join(store, pkgId))
}

function rimraf (filePath: string) {
  return new Promise((resolve, reject) => {
    cbRimraf(filePath, (err: Error) => err ? reject(err) : resolve())
  })
}
