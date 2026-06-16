import type { Stats } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'

import { checkbox } from '@inquirer/prompts'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/installing.commands'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { isSubdirectory } from './isSubdirectory.js'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return { ...rcOptionsTypes() }
}

export const commandNames = ['patch-remove']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Remove existing patch files',
    url: docsUrl('patch-remove'),
    usages: ['pnpm patch-remove [pkg...]'],
  })
}

export type PatchRemoveCommandOptions = install.InstallCommandOptions & Pick<Config, 'dir' | 'lockfileDir' | 'patchesDir' | 'patchedDependencies'> & Pick<ConfigContext, 'rootProjectManifest'>

export async function handler (opts: PatchRemoveCommandOptions, params: string[]): Promise<void> {
  let patchesToRemove = params
  const patchedDependencies = { ...opts.patchedDependencies }

  if (!params.length) {
    const allPatches = Object.keys(patchedDependencies)
    if (allPatches.length) {
      try {
        patchesToRemove = await checkbox({
          choices: allPatches.map((name) => ({ name, value: name })),
          message: 'Select the patch to be removed',
          required: true,
          validate: (values) => {
            return values.length === 0 ? 'Select at least one option.' : true
          },
          theme: { keybindings: ['vim'] },
        })
      } catch (err: unknown) {
        if (err instanceof Error && err.name === 'ExitPromptError') {
          throw new PnpmError('PATCH_REMOVE_CANCELED', 'Canceled')
        }
        throw err
      }
    }
  }

  if (!patchesToRemove.length) {
    throw new PnpmError('NO_PATCHES_TO_REMOVE', 'There are no patches that need to be removed')
  }

  for (const patch of patchesToRemove) {
    if (!Object.hasOwn(patchedDependencies, patch)) {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch "${patch}" not found in patched dependencies`)
    }
  }

  const patchRemovalContext = await getPatchRemovalContext(opts)
  const patchesToRemoveTargets = await Promise.all(patchesToRemove.map(async (patch) => {
    const patchFile = patchedDependencies[patch]
    if (patchFile == null) {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch "${patch}" not found in patched dependencies`)
    }
    return getPatchRemovalTarget(patch, patchFile, patchRemovalContext)
  }))

  await Promise.all(patchesToRemoveTargets.map(unlinkPatchIfExists))
  for (const { patch } of patchesToRemoveTargets) {
    delete patchedDependencies[patch]
  }

  const patchesDirs = new Set(patchesToRemoveTargets.map(({ parentDir }) => parentDir))
  await Promise.all(Array.from(patchesDirs).map(async (dir) => {
    try {
      const files = await fs.readdir(dir)
      if (!files.length) {
        await fs.rmdir(dir)
      }
    } catch {}
  }))
  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })

  await install.handler({
    ...opts,
    patchedDependencies,
  })
}

interface PatchRemovalContext {
  lockfileDir: string
  patchesDir: string
  realPatchesDir?: string
}

interface PatchRemovalTarget {
  patch: string
  patchFile: string
  parentDir: string
  targetPath: string
  targetExists: boolean
}

async function getPatchRemovalContext (opts: PatchRemoveCommandOptions): Promise<PatchRemovalContext> {
  const lockfileDir = path.resolve(opts.lockfileDir ?? opts.dir ?? process.cwd())
  const realLockfileDir = await fs.realpath(lockfileDir)
  const patchesDirSetting = opts.patchesDir ?? 'patches'
  const patchesDir = path.join(lockfileDir, path.normalize(patchesDirSetting))

  if (!isSubdirectory(lockfileDir, patchesDir)) {
    throw new PnpmError('PATCHES_DIR_OUTSIDE_PROJECT', `The configured patches directory is outside the project: ${patchesDirSetting}`)
  }

  const realPatchesDir = await realpathIfExists(patchesDir)
  if (realPatchesDir != null && !isSubdirectory(realLockfileDir, realPatchesDir)) {
    throw new PnpmError('PATCHES_DIR_OUTSIDE_PROJECT', `The configured patches directory is outside the project: ${patchesDirSetting}`)
  }

  return {
    lockfileDir,
    patchesDir,
    realPatchesDir,
  }
}

async function getPatchRemovalTarget (
  patch: string,
  patchFile: string,
  ctx: PatchRemovalContext
): Promise<PatchRemovalTarget> {
  const targetPath = path.resolve(ctx.lockfileDir, patchFile)
  if (
    targetPath === ctx.patchesDir ||
    !isSubdirectory(ctx.patchesDir, targetPath)
  ) {
    throw new PnpmError('PATCH_FILE_OUTSIDE_PATCHES_DIR', `Patch file "${patchFile}" is outside the configured patches directory`)
  }

  const parentDir = path.dirname(targetPath)
  const targetStats = await lstatIfExists(targetPath)
  const realParentDir = await realpathIfExists(parentDir)
  const realPatchesDir = ctx.realPatchesDir ?? (await realpathIfExists(ctx.patchesDir))
  if (
    realParentDir != null &&
    realPatchesDir != null &&
    !isSubdirectory(realPatchesDir, realParentDir)
  ) {
    throw new PnpmError('PATCH_FILE_OUTSIDE_PATCHES_DIR', `Patch file "${patchFile}" is outside the configured patches directory`)
  }
  if (targetStats?.isDirectory()) {
    throw new PnpmError('PATCH_FILE_IS_DIRECTORY', `Patch file "${patchFile}" is a directory`)
  }

  return {
    patch,
    patchFile,
    parentDir,
    targetPath,
    targetExists: targetStats != null,
  }
}

async function unlinkPatchIfExists ({ targetExists, targetPath }: PatchRemovalTarget): Promise<void> {
  if (!targetExists) return

  try {
    await fs.unlink(targetPath)
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT')) return
    throw err
  }
}

async function lstatIfExists (targetPath: string): Promise<Stats | undefined> {
  try {
    return await fs.lstat(targetPath)
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT')) return undefined
    throw err
  }
}

async function realpathIfExists (targetPath: string): Promise<string | undefined> {
  try {
    return await fs.realpath(targetPath)
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT')) return undefined
    throw err
  }
}

function isErrorWithCode (err: unknown, code: string): err is NodeJS.ErrnoException {
  return typeof err === 'object' && err !== null && 'code' in err && err.code === code
}
