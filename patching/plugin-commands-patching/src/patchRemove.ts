import path from 'node:path'
import fs from 'node:fs/promises'
import { docsUrl } from '@pnpm/cli-utils'
import { install } from '@pnpm/plugin-commands-installation'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { prompt } from 'enquirer'
import pick from 'ramda/src/pick'
import { updatePatchedDependencies } from './updatePatchedDependencies'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return { ...rcOptionsTypes() }
}

export const commandNames = ['patch-remove']

export function help (): string {
  return renderHelp({
    description: 'Remove existing patch files',
    url: docsUrl('patch-remove'),
    usages: ['pnpm patch-remove [pkg...]'],
  })
}

export type PatchRemoveCommandOptions = install.InstallCommandOptions & Pick<Config, 'dir' | 'lockfileDir' | 'patchesDir' | 'rootProjectManifest' | 'patchedDependencies'>

export async function handler (opts: PatchRemoveCommandOptions, params: string[]): Promise<void> {
  let patchesToRemove = params
  const patchedDependencies = opts.patchedDependencies ?? {}

  if (!params.length) {
    const allPatches = Object.keys(patchedDependencies)
    if (allPatches.length) {
      ({ patches: patchesToRemove } = await prompt<{
        patches: string[]
      }>({
        type: 'multiselect',
        name: 'patches',
        message: 'Select the patch to be removed',
        choices: allPatches,
        validate (value) {
          return value.length === 0 ? 'Select at least one option.' : true
        },
      }))
    }
  }

  if (!patchesToRemove.length) {
    throw new PnpmError('NO_PATCHES_TO_REMOVE', 'There are no patches that need to be removed')
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
