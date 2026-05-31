import fs from 'node:fs/promises'
import path from 'node:path'

import { checkbox } from '@inquirer/prompts'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/installing.commands'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

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
  const patchedDependencies = opts.patchedDependencies ?? {}

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

  const patchesDirs = new Set<string>()
  await Promise.all(patchesToRemove.map(async (patch) => {
    if (Object.hasOwn(patchedDependencies, patch)) {
      const patchFile = patchedDependencies[patch]
      patchesDirs.add(path.dirname(patchFile))
      await fs.rm(patchFile, { force: true })
      delete patchedDependencies![patch]
    }
  }))

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

  return install.handler(opts)
}
