import rimraf = require('rimraf-then')
import path = require('path')
import binify from './binify'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'

export default async function removeTopDependency (dependencyName: string, modules: string) {
  return Promise.all([
    removeBins(dependencyName, modules),
    rimraf(path.join(modules, dependencyName)),
  ])
}

async function removeBins (uninstalledPkg: string, modules: string) {
  const uninstalledPkgPath = path.join(modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)
  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)
  return Promise.all(
    cmds.map(cmd => path.join(modules, '.bin', cmd.name)).map(rimraf)
  )
}
