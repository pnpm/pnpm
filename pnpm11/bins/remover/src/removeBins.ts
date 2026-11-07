import path from 'node:path'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import {
  removalLogger,
} from '@pnpm/core-loggers'
import { isError } from '@pnpm/error'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { cmdExtension as CMD_EXTENSION } from 'cmd-extension'
import isWindows from 'is-windows'

async function removeOnWin (cmd: string): Promise<void> {
  removalLogger.debug(cmd)
  const results = await Promise.allSettled([
    rimraf(cmd),
    rimraf(`${cmd}.ps1`),
    rimraf(`${cmd}${CMD_EXTENSION}`),
    // The `node` bin is linked as a real `<cmd>.exe` (see the node
    // special-case in `@pnpm/bins.linker`), so it must be removed too —
    // otherwise a stale `node.exe` survives on PATH after uninstall.
    rimraf(`${cmd}.exe`),
  ])
  const errors = results.flatMap((result) => result.status === 'rejected' ? [result.reason] : [])
  if (errors.length === 1) throw errors[0]
  if (errors.length > 1) {
    throw new AggregateError(errors, `Failed to remove Windows bin shims for ${cmd}`)
  }
}

async function removeOnNonWin (binPath: string): Promise<void> {
  removalLogger.debug(binPath)
  return rimraf(binPath)
}

export const removeBin: (cmd: string) => Promise<void> = isWindows() ? removeOnWin : removeOnNonWin

export async function removeBinsOfDependency (
  dependencyDir: string,
  opts: {
    dryRun?: boolean
    binsDir: string
  }
): Promise<DependencyManifest | undefined> {
  const uninstalledPkgJson = await safeReadDependencyManifest(dependencyDir)
  if (!uninstalledPkgJson) return
  const cmds = await getBinsFromPackageManifest(uninstalledPkgJson, dependencyDir)

  if (!opts.dryRun) {
    await Promise.all(
      cmds
        .map((cmd) => path.join(opts.binsDir, cmd.name))
        .map(removeBin)
    )
  }

  return uninstalledPkgJson
}

/**
 * Returns `undefined` when the manifest is missing or malformed, or its path
 * runs through a file, so that its bins are left in place instead of blocking
 * the dependency's removal. Rethrows every other read error.
 */
export async function safeReadDependencyManifest (dependencyDir: string): Promise<DependencyManifest | undefined> {
  try {
    return (await safeReadPackageJsonFromDir(dependencyDir) as DependencyManifest | null) ?? undefined
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ERR_PNPM_BAD_PACKAGE_JSON' || err.code === 'ENOTDIR')) {
      return undefined
    }
    throw err
  }
}
