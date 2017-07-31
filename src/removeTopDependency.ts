import rimraf = require('rimraf-then')
import path = require('path')
import binify from './binify'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {rootLogger} from 'pnpm-logger'

export default async function removeTopDependency (
  dependency: {
    name: string,
    dev: boolean,
    optional: boolean,
  },
  modules: string
) {
  const results = await Promise.all([
    removeBins(dependency.name, modules),
    rimraf(path.join(modules, dependency.name)),
  ])

  const uninstalledPkg = results[0]
  rootLogger.info({
    removed: {
      name: dependency.name,
      version: uninstalledPkg && uninstalledPkg.version,
      dependencyType: dependency.dev && 'dev' || dependency.optional && 'optional' || 'prod'
    },
  })
}

async function removeBins (uninstalledPkg: string, modules: string) {
  const uninstalledPkgPath = path.join(modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)
  await Promise.all(
    cmds.map(cmd => path.join(modules, '.bin', cmd.name)).map(rimraf)
  )

  return uninstalledPkgJson
}
