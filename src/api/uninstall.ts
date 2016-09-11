import cbRimraf = require('rimraf')
import path = require('path')

import initCmd, {CommandNamespace} from './init_cmd'
import getSaveType from '../get_save_type'
import removeDeps from '../remove_deps'
import binify from '../binify'
import defaults from '../defaults'
import requireJson from '../fs/require_json'
import {PublicInstallationOptions} from './install'

export default async function uninstallCmd (pkgsToUninstall: string[], opts: PublicInstallationOptions) {
  opts = Object.assign({}, defaults, opts)

  const uninstalledPkgs: string[] = []
  const saveType = getSaveType(opts)
  let cmd: CommandNamespace

  try {
    cmd = await initCmd(opts)
    cmd.pkg.pkg.dependencies = cmd.pkg.pkg.dependencies || {}
    const pkgFullNames = pkgsToUninstall.map(dep => cmd.ctx.dependencies[cmd.pkg.path].find(_ => _.indexOf(dep + '@') === 0))
    tryUninstall(pkgFullNames.slice())
    if (cmd.ctx.dependencies[cmd.pkg.path]) {
      pkgFullNames.forEach(dep => {
        cmd.ctx.dependencies[cmd.pkg.path].splice(cmd.ctx.dependencies[cmd.pkg.path].indexOf(dep), 1)
      })
      if (!cmd.ctx.dependencies[cmd.pkg.path].length) {
        delete cmd.ctx.dependencies[cmd.pkg.path]
      }
    }
    await Promise.all(uninstalledPkgs.map(removePkgFromStore))

    cmd.storeJsonCtrl.save({
      pnpm: cmd.ctx.pnpm,
      dependents: cmd.ctx.dependents,
      dependencies: cmd.ctx.dependencies
    })
    await Promise.all(pkgsToUninstall.map(dep => rimraf(path.join(cmd.ctx.root, 'node_modules', dep))))
    if (saveType) {
      await removeDeps(cmd.pkg.path, pkgsToUninstall, saveType)
    }
    await cmd.unlock()
  } catch (err) {
    if (cmd && cmd.unlock) await cmd.unlock()
    throw err
  }

  function canBeUninstalled (pkgFullName: string) {
    return !cmd.ctx.dependents[pkgFullName] || !cmd.ctx.dependents[pkgFullName].length ||
      cmd.ctx.dependents[pkgFullName].length === 1 && cmd.ctx.dependents[pkgFullName].indexOf(cmd.pkg.path) !== -1
  }

  function tryUninstall (pkgFullNames: string[]) {
    let numberOfUninstalls: number
    do {
      numberOfUninstalls = 0
      for (let i = 0; i < pkgFullNames.length; ) {
        if (canBeUninstalled(pkgFullNames[i])) {
          const uninstalledPkg = pkgFullNames.splice(i, 1)[0]
          removeBins(uninstalledPkg)
          uninstalledPkgs.push(uninstalledPkg)
          const deps = cmd.ctx.dependencies[uninstalledPkg] || []
          delete cmd.ctx.dependencies[uninstalledPkg]
          delete cmd.ctx.dependents[uninstalledPkg]
          deps.forEach((dep: string) => removeDependency(dep, uninstalledPkg))
          tryUninstall(deps)
          numberOfUninstalls++
          continue
        }
        i++
      }
    } while (numberOfUninstalls)
  }

  function removeDependency (dependentPkgName: string, uninstalledPkg: string) {
    if (!cmd.ctx.dependents[dependentPkgName]) return
    cmd.ctx.dependents[dependentPkgName].splice(cmd.ctx.dependents[dependentPkgName].indexOf(uninstalledPkg), 1)
    if (!cmd.ctx.dependents[dependentPkgName].length) {
      delete cmd.ctx.dependents[dependentPkgName]
    }
  }

  function removeBins (uninstalledPkg: string) {
    const uninstalledPkgJson = requireJson(path.join(cmd.ctx.store, uninstalledPkg, '_/package.json'))
    const bins = binify(uninstalledPkgJson)
    Object.keys(bins).forEach(bin => cbRimraf.sync(path.join(cmd.ctx.root, 'node_modules/.bin', bin)))
  }

  function removePkgFromStore (pkgFullName: string) {
    return rimraf(path.join(cmd.ctx.store, pkgFullName))
  }

  function rimraf (filePath: string) {
    return new Promise((resolve, reject) => {
      cbRimraf(filePath, (err: Error) => err ? reject(err) : resolve())
    })
  }
}
