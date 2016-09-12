import cbRimraf = require('rimraf')
import path = require('path')

import initCmd, {CommandNamespace} from './init_cmd'
import getSaveType from '../get_save_type'
import removeDeps from '../remove_deps'
import binify from '../binify'
import defaults from '../defaults'
import requireJson from '../fs/require_json'
import {PublicInstallationOptions} from './install'
import {StoreJson} from '../fs/store_json_controller'

export default async function uninstallCmd (pkgsToUninstall: string[], opts: PublicInstallationOptions) {
  opts = Object.assign({}, defaults, opts)

  const saveType = getSaveType(opts)
  const cmd: CommandNamespace = await initCmd(opts)

  try {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot uninstall')
    }
    const pkg = cmd.pkg // NOTE: otherwise TypeScript thinks cmd.pkg might be undefined for some reason
    cmd.pkg.pkg.dependencies = cmd.pkg.pkg.dependencies || {}

    // this is OK. The store might not have records for the package
    // maybe it was cloned, `pnpm install` was not executed
    // and remove is done on a package with no dependencies installed
    cmd.ctx.storeJson.dependencies[cmd.pkg.path] = cmd.ctx.storeJson.dependencies[cmd.pkg.path] || []

    const pkgFullNames = <string[]>pkgsToUninstall
      .map(dep => cmd.ctx.storeJson.dependencies[pkg.path].find(_ => _.indexOf(`${dep}@`) === 0))
      .filter(pkgFullName => !!pkgFullName)
    const uninstalledPkgs = tryUninstall(pkgFullNames.slice(), cmd.ctx.storeJson, cmd.pkg.path)
    uninstalledPkgs.forEach(uninstalledPkg => removeBins(uninstalledPkg, cmd.ctx.store, cmd.ctx.root))
    if (cmd.ctx.storeJson.dependencies[cmd.pkg.path]) {
      pkgFullNames.forEach(dep => {
        cmd.ctx.storeJson.dependencies[pkg.path].splice(cmd.ctx.storeJson.dependencies[pkg.path].indexOf(dep), 1)
      })
      if (!cmd.ctx.storeJson.dependencies[cmd.pkg.path].length) {
        delete cmd.ctx.storeJson.dependencies[cmd.pkg.path]
      }
    }
    await Promise.all(uninstalledPkgs.map(pkgFullName => removePkgFromStore(pkgFullName, cmd.ctx.store)))

    cmd.storeJsonCtrl.save(cmd.ctx.storeJson)
    await Promise.all(pkgsToUninstall.map(dep => rimraf(path.join(cmd.ctx.root, 'node_modules', dep))))
    if (saveType) {
      await removeDeps(cmd.pkg.path, pkgsToUninstall, saveType)
    }
    await cmd.unlock()
  } catch (err) {
    if (typeof cmd !== 'undefined' && cmd.unlock) await cmd.unlock()
    throw err
  }
}

function canBeUninstalled (pkgFullName: string, storeJson: StoreJson, pkgPath: string) {
  return !storeJson.dependents[pkgFullName] || !storeJson.dependents[pkgFullName].length ||
    storeJson.dependents[pkgFullName].length === 1 && storeJson.dependents[pkgFullName].indexOf(pkgPath) !== -1
}

function tryUninstall (pkgFullNames: string[], storeJson: StoreJson, pkgPath: string) {
  const uninstalledPkgs: string[] = []
  let numberOfUninstalls: number
  do {
    numberOfUninstalls = 0
    for (let i = 0; i < pkgFullNames.length; ) {
      if (canBeUninstalled(pkgFullNames[i], storeJson, pkgPath)) {
        const uninstalledPkg = pkgFullNames.splice(i, 1)[0]
        uninstalledPkgs.push(uninstalledPkg)
        const deps = storeJson.dependencies[uninstalledPkg] || []
        delete storeJson.dependencies[uninstalledPkg]
        delete storeJson.dependents[uninstalledPkg]
        deps.forEach((dep: string) => removeDependency(dep, uninstalledPkg, storeJson))
        Array.prototype.push.apply(uninstalledPkgs, tryUninstall(deps, storeJson, pkgPath))
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

function removePkgFromStore (pkgFullName: string, store: string) {
  return rimraf(path.join(store, pkgFullName))
}

function rimraf (filePath: string) {
  return new Promise((resolve, reject) => {
    cbRimraf(filePath, (err: Error) => err ? reject(err) : resolve())
  })
}
