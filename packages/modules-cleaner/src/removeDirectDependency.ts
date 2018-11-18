import {
  removalLogger,
  rootLogger,
} from '@pnpm/core-loggers'
import binify from '@pnpm/package-bins'
import { DependenciesField } from '@pnpm/types'
import { safeReadPackageFromDir } from '@pnpm/utils'
import path = require('path')
import rimraf = require('rimraf-then')

export default async function removeDirectDependency (
  dependency: {
    dependenciesField?: DependenciesField | undefined,
    name: string,
  },
  opts: {
    bin: string,
    dryRun?: boolean,
    modulesDir: string,
    muteLogs?: boolean,
    prefix: string,
  },
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && remove(path.join(opts.modulesDir, dependency.name)),
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.debug({
      prefix: opts.prefix,
      removed: {
        dependencyType: dependency.dependenciesField === 'devDependencies' && 'dev' ||
          dependency.dependenciesField === 'optionalDependencies' && 'optional' ||
          dependency.dependenciesField === 'dependencies' && 'prod' ||
          undefined,
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
    modulesDir: string,
    bin: string,
  },
) {
  const uninstalledPkgPath = path.join(opts.modulesDir, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPackageFromDir(uninstalledPkgPath)

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
