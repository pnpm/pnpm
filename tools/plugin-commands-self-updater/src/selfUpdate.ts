import path from 'node:path'
import { docsUrl } from '@pnpm/cli-utils'
import { packageManager, isExecutedByCorepack } from '@pnpm/cli-meta'
import { createResolver } from '@pnpm/client'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { linkBins } from '@pnpm/link-bins'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { installPnpmToTools } from './installPnpmToTools'

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

export type SelfUpdateCommandOptions = Pick<Config,
| 'cacheDir'
| 'dir'
| 'lockfileDir'
| 'managePackageManagerVersions'
| 'modulesDir'
| 'pnpmHomeDir'
| 'rawConfig'
| 'registries'
| 'rootProjectManifestDir'
| 'wantedPackageManager'
>

export async function handler (
  opts: SelfUpdateCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_SELF_UPDATE_IN_COREPACK', 'You should update pnpm with corepack')
  }
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  const pkgName = 'pnpm'
  const bareSpecifier = params[0] ?? 'latest'
  const resolution = await resolve({ alias: pkgName, bareSpecifier }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot find "${bareSpecifier}" version of pnpm`)
  }

  if (opts.wantedPackageManager?.name === packageManager.name && opts.managePackageManagerVersions) {
    if (opts.wantedPackageManager?.version !== resolution.manifest.version) {
      const { manifest, writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
      manifest.packageManager = `pnpm@${resolution.manifest.version}`
      await writeProjectManifest(manifest)
      return `The current project has been updated to use pnpm v${resolution.manifest.version}`
    } else {
      return `The current project is already set to use pnpm v${resolution.manifest.version}`
    }
  }
  if (resolution.manifest.version === packageManager.version) {
    return `The currently active ${packageManager.name} v${packageManager.version} is already "${bareSpecifier}" and doesn't need an update`
  }

  const { baseDir, alreadyExisted } = await installPnpmToTools(resolution.manifest.version, opts)
  await linkBins(path.join(baseDir, 'node_modules'), opts.pnpmHomeDir,
    {
      warn: globalWarn,
    }
  )
  return alreadyExisted
    ? `The ${bareSpecifier} version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${baseDir}.`
    : undefined
}
