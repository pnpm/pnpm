import rimraf = require('rimraf-then')
import path = require('path')
import binify from './binify'
import safeReadPkg from './fs/safeReadPkg'

export default async function removeTopDependency (dependencyName: string, modules: string) {
  return Promise.all([
    rimraf(path.join(modules, dependencyName)),
    removeBins(dependencyName, modules),
  ])
}

async function removeBins (uninstalledPkg: string, modules: string) {
  const uninstalledPkgPath = path.join(modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkg(uninstalledPkgPath)
  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)
  return Promise.all(
    cmds.map(cmd => path.join(modules, '.bin', cmd.name)).map(rimraf)
  )
}
