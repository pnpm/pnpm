import {
  removalLogger,
  rootLogger,
} from '@pnpm/core-loggers'
import binify from '@pnpm/package-bins'
import path = require('path')
import rimraf = require('rimraf-then')
import { fromDir as safeReadPkgFromDir } from './safeReadPkg'

export default async function removeTopDependency (
  dependency: {
    dev: boolean,
    name: string,
    optional: boolean,
  },
  opts: {
    bin: string,
    dryRun?: boolean,
    importerNModulesDir: string,
    muteLogs?: boolean,
    prefix: string,
  },
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && remove(path.join(opts.importerNModulesDir, dependency.name)),
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.debug({
      prefix: opts.prefix,
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
    importerNModulesDir: string,
    bin: string,
  },
) {
  const uninstalledPkgPath = path.join(opts.importerNModulesDir, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPkgFromDir(uninstalledPkgPath)

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)

  if (!opts.dryRun) {
    // TODO: what about the .cmd bin files on Windows?
    await Promise.all(
      cmds
        .map((cmd) => path.join(opts.bin, cmd.name))
        .map(remove),
    )
  }

  return uninstalledPkgJson
}

function remove (p: string) {
  removalLogger.debug(p)
  return rimraf(p)
}
