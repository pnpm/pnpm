import path from 'node:path'

import isWindows from 'is-windows'
import rimraf from '@zkochan/rimraf'
import CMD_EXTENSION from 'cmd-extension'

import { removalLogger } from '@pnpm/core-loggers'
import type { DependencyManifest } from '@pnpm/types'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'

async function removeOnWin(cmd: string): Promise<void> {
  removalLogger.debug(cmd)

  await Promise.all([
    rimraf(cmd),
    rimraf(`${cmd}.ps1`),
    rimraf(`${cmd}${CMD_EXTENSION}`),
  ])
}

async function removeOnNonWin(p: string): Promise<void> {
  removalLogger.debug(p)

  return rimraf(p)
}

export const removeBin = isWindows() ? removeOnWin : removeOnNonWin

export async function removeBinsOfDependency(
  dependencyDir: string,
  opts: {
    dryRun?: boolean | undefined
    binsDir?: string | undefined
  }
): Promise<DependencyManifest | undefined> {
  const uninstalledPkgJson: DependencyManifest | null = (await safeReadPackageJsonFromDir(
    dependencyDir
  ))

  if (!uninstalledPkgJson) {
    return
  }

  const cmds = await getBinsFromPackageManifest(
    uninstalledPkgJson,
    dependencyDir
  )

  if (!opts.dryRun) {
    await Promise.all(
      cmds.map((cmd: {
        name: string;
        path: string;
      }): string => {
        return path.join(opts.binsDir ?? '', cmd.name);
      }).map(removeBin)
    )
  }

  return uninstalledPkgJson
}
