import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { getCurrentPackageName, packageManager, isExecutedByCorepack } from '@pnpm/cli-meta'
import { createResolver } from '@pnpm/client'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { add, type InstallCommandOptions } from '@pnpm/plugin-commands-installation'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { getToolDirPath } from '@pnpm/tools.path'
import { linkBins } from '@pnpm/link-bins'
import { sync as rimraf } from '@zkochan/rimraf'
import { fastPathTemp as pathTemp } from 'path-temp'
import pick from 'ramda/src/pick'
import renameOverwrite from 'rename-overwrite'
import renderHelp from 'render-help'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export const commandNames = ['self-update']

export function help (): string {
  return renderHelp({
    description: 'Updates pnpm to the latest version (or the one specified)',
    descriptionLists: [],
    url: docsUrl('self-update'),
    usages: [
      'pnpm self-update',
      'pnpm self-update 9',
      'pnpm self-update next-10',
      'pnpm self-update 9.10.0',
    ],
  })
}

export type SelfUpdateCommandOptions = InstallCommandOptions & Pick<Config, 'wantedPackageManager' | 'managePackageManagerVersions'>

export async function handler (
  opts: SelfUpdateCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_SELF_UPDATE_IN_COREPACK', 'You should update pnpm with corepack')
  }
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  const pkgName = 'pnpm'
  const pref = params[0] ?? 'latest'
  const resolution = await resolve({ alias: pkgName, pref }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: pickRegistryForPackage(opts.registries, pkgName, pref),
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot find "${pref}" version of pnpm`)
  }
  if (resolution.manifest.version === packageManager.version) {
    return `The currently active ${packageManager.name} v${packageManager.version} is already "${pref}" and doesn't need an update`
  }

  if (opts.wantedPackageManager?.name === packageManager.name && opts.managePackageManagerVersions) {
    const { manifest, writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
    manifest.packageManager = `pnpm@${resolution.manifest.version}`
    await writeProjectManifest(manifest)
    return `The current project has been updated to use pnpm v${resolution.manifest.version}`
  }

  const currentPkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: resolution.manifest.version,
    },
  })
  const alreadyExists = fs.existsSync(dir)
  if (!alreadyExists) {
    const stage = pathTemp(dir)
    fs.mkdirSync(stage, { recursive: true })
    fs.writeFileSync(path.join(stage, 'package.json'), '{}')
    try {
      await add.handler(
        {
          ...opts,
          dir: stage,
          lockfileDir: stage,
          // We want to avoid symlinks because of the rename step,
          // which breaks the junctions on Windows.
          nodeLinker: 'hoisted',
          // This won't be used but there is currently no way to skip the bin creation
          // and we can't create the bin shims in the pnpm home directory
          // because the stage directory will be renamed.
          bin: path.join(stage, 'node_modules/.bin'),
        },
        [`${currentPkgName}@${resolution.manifest.version}`]
      )
      renameOverwrite.sync(stage, dir)
    } catch (err: unknown) {
      try {
        rimraf(stage)
      } catch {} // eslint-disable-line:no-empty
      throw err
    }
  }
  await linkBins(path.join(dir, opts.modulesDir ?? 'node_modules'), opts.pnpmHomeDir,
    {
      warn: globalWarn,
    }
  )
  return alreadyExists
    ? `The ${pref} version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${dir}.`
    : undefined
}
