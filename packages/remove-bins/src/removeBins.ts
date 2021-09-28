import path from 'path'
import {
  removalLogger,
} from '@pnpm/core-loggers'
import binify from '@pnpm/package-bins'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import { DependencyManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import CMD_EXTENSION from 'cmd-extension'
import isWindows from 'is-windows'

async function removeOnWin (cmd: string) {
  removalLogger.debug(cmd)
  await Promise.all([
    rimraf(cmd),
    rimraf(`${cmd}.ps1`),
    rimraf(`${cmd}${CMD_EXTENSION}`),
  ])
}

async function removeOnNonWin (p: string) {
  removalLogger.debug(p)
  return rimraf(p)
}

export const removeBin = isWindows() ? removeOnWin : removeOnNonWin

export async function removeBinsOfDependency (
  dependencyDir: string,
  opts: {
    dryRun?: boolean
    binsDir: string
  }
) {
  const uninstalledPkgJson = await safeReadPackageFromDir(dependencyDir) as DependencyManifest

  if (!uninstalledPkgJson) return
  const cmds = await binify(uninstalledPkgJson, dependencyDir)

  if (!opts.dryRun) {
    await Promise.all(
      cmds
        .map((cmd) => path.join(opts.binsDir, cmd.name))
        .map(removeBin)
    )
  }

  return uninstalledPkgJson
}
