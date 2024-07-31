import path from 'path'
import fs from 'fs/promises'
import { docsUrl } from '@pnpm/cli-utils'
import { install } from '@pnpm/plugin-commands-installation'
import { type Config, types as allTypes } from '@pnpm/config'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { PnpmError } from '@pnpm/error'
import { type ProjectRootDir } from '@pnpm/types'
import renderHelp from 'render-help'
import { prompt } from 'enquirer'
import pick from 'ramda/src/pick'

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

export type PatchRemoveCommandOptions = install.InstallCommandOptions & Pick<Config, 'dir' | 'lockfileDir' | 'patchesDir' | 'rootProjectManifest'>

export async function handler (opts: PatchRemoveCommandOptions, params: string[]): Promise<void> {
  let patchesToRemove = params
  const lockfileDir = (opts.lockfileDir ?? opts.dir ?? process.cwd()) as ProjectRootDir
  const { writeProjectManifest, manifest } = await tryReadProjectManifest(lockfileDir)
  const rootProjectManifest = opts.rootProjectManifest ?? manifest ?? {}
  const patchedDependencies = rootProjectManifest.pnpm?.patchedDependencies ?? {}

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
    if (Object.prototype.hasOwnProperty.call(patchedDependencies, patch)) {
      const patchFile = path.join(lockfileDir, patchedDependencies[patch])
      patchesDirs.add(path.dirname(patchFile))
      await fs.rm(patchFile, { force: true })
      delete rootProjectManifest.pnpm!.patchedDependencies![patch]
      if (!Object.keys(rootProjectManifest.pnpm!.patchedDependencies!).length) {
        delete rootProjectManifest.pnpm!.patchedDependencies
        if (!Object.keys(rootProjectManifest.pnpm!).length) {
          delete rootProjectManifest.pnpm
        }
      }
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

  await writeProjectManifest(rootProjectManifest)

  if (opts?.selectedProjectsGraph?.[lockfileDir]) {
    opts.selectedProjectsGraph[lockfileDir].package.manifest = rootProjectManifest
  }

  if (opts?.allProjectsGraph?.[lockfileDir].package.manifest) {
    opts.allProjectsGraph[lockfileDir].package.manifest = rootProjectManifest
  }

  return install.handler(opts)
}
