import rimraf = require('rimraf-then')
import path = require('path')
import binify from './binify'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {rootLogger} from './loggers'

export default async function removeTopDependency (
  dependency: {
    name: string,
    dev: boolean,
    optional: boolean,
  },
  opts: {
    modules: string,
    bin: string,
  }
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    rimraf(path.join(opts.modules, dependency.name)),
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

async function removeBins (
  uninstalledPkg: string,
  opts: {
    modules: string,
    bin: string,
  }
) {
  const uninstalledPkgPath = path.join(opts.modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)
  await Promise.all(
    cmds.map(cmd => path.join(opts.bin, cmd.name)).map(rimraf)
  )

  return uninstalledPkgJson
}
