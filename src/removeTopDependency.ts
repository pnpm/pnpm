import path = require('path')
import rimraf = require('rimraf-then')
import binify from './binify'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {rootLogger} from './loggers'

export default async function removeTopDependency (
  dependency: {
    dev: boolean,
    name: string,
    optional: boolean,
  },
  opts: {
    bin: string,
    dryRun?: boolean,
    modules: string,
    muteLogs?: boolean,
  },
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && rimraf(path.join(opts.modules, dependency.name)),
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.info({
      removed: {
        dependencyType: dependency.dev && 'dev' || dependency.optional && 'optional' || 'prod',
        name: dependency.name,
        version: uninstalledPkg && uninstalledPkg.version,
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
  },
) {
  const uninstalledPkgPath = path.join(opts.modules, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)

  if (!opts.dryRun) {
    await Promise.all(
      cmds.map((cmd) => path.join(opts.bin, cmd.name)).map(rimraf),
    )
  }

  return uninstalledPkgJson
}
