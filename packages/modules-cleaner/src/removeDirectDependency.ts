import {
  removalLogger,
  rootLogger,
} from '@pnpm/core-loggers'
import binify from '@pnpm/package-bins'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import { DependenciesField, DependencyManifest } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import isWindows = require('is-windows')
import path = require('path')

export default async function removeDirectDependency (
  dependency: {
    dependenciesField?: DependenciesField | undefined,
    name: string,
  },
  opts: {
    binsDir: string,
    dryRun?: boolean,
    modulesDir: string,
    muteLogs?: boolean,
    rootDir: string,
  },
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && remove(path.join(opts.modulesDir, dependency.name)) as any, // tslint:disable-line:no-any
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.debug({
      prefix: opts.rootDir,
      removed: {
        dependencyType: dependency.dependenciesField === 'devDependencies' && 'dev' ||
          dependency.dependenciesField === 'optionalDependencies' && 'optional' ||
          dependency.dependenciesField === 'dependencies' && 'prod' ||
          undefined,
        name: dependency.name,
        version: uninstalledPkg?.version,
      },
    })
  }
}

async function removeOnWin (cmd: string) {
  removalLogger.debug(cmd)
  await Promise.all([
    rimraf(cmd),
    rimraf(`${cmd}.ps1`),
    rimraf(`${cmd}.cmd`),
  ])
}

function removeOnNonWin (p: string) {
  removalLogger.debug(p)
  return rimraf(p)
}

const remove = isWindows() ? removeOnWin : removeOnNonWin

async function removeBins (
  uninstalledPkg: string,
  opts: {
    dryRun?: boolean,
    modulesDir: string,
    binsDir: string,
  },
) {
  const uninstalledPkgPath = path.join(opts.modulesDir, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPackageFromDir(uninstalledPkgPath) as DependencyManifest

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)

  if (!opts.dryRun) {
    await Promise.all(
      cmds
        .map((cmd) => path.join(opts.binsDir, cmd.name))
        .map(remove),
    )
  }

  return uninstalledPkgJson
}
