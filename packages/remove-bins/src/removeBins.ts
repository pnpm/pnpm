import {
  removalLogger,
} from '@pnpm/core-loggers'
import binify from '@pnpm/package-bins'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import { DependencyManifest } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import isWindows = require('is-windows')
import path = require('path')

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

export const remove = isWindows() ? removeOnWin : removeOnNonWin

export async function removeBins (
  uninstalledPkg: string,
  opts: {
    dryRun?: boolean,
    modulesDir: string,
    binsDir: string,
  }
) {
  const uninstalledPkgPath = path.join(opts.modulesDir, uninstalledPkg)
  const uninstalledPkgJson = await safeReadPackageFromDir(uninstalledPkgPath) as DependencyManifest

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, uninstalledPkgPath)

  if (!opts.dryRun) {
    await Promise.all(
      cmds
        .map((cmd) => path.join(opts.binsDir, cmd.name))
        .map(remove)
    )
  }

  return uninstalledPkgJson
}
