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
    dryRun?: boolean,
    modules: string,
    bin: string,
    muteLogs?: boolean,
  }
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && rimraf(path.join(opts.modules, dependency.name)),
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.info({
      removed: {
        name: dependency.name,
        version: uninstalledPkg && uninstalledPkg.version,
        dependencyType: dependency.dev && 'dev' || dependency.optional && 'optional' || 'prod'
      },
    })
  }
}

async function removeBins (
  uninstalledPkg: string,
  opts: {
    dryRun?: boolean,
    modules: string,
    bin: string,
  }
) {
  const uninstalledPkgPath = path.join(opts.modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)

  if (!opts.dryRun) {
    await Promise.all(
      cmds.map(cmd => path.join(opts.bin, cmd.name)).map(rimraf)
    )
  }

  return uninstalledPkgJson
}
