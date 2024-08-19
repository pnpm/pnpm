import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { getCurrentPackageName, packageManager, isExecutedByCorepack } from '@pnpm/cli-meta'
import { createResolver } from '@pnpm/client'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { add, type InstallCommandOptions } from '@pnpm/plugin-commands-installation'
import { getToolDirPath } from '@pnpm/tools.path'
import { linkBins } from '@pnpm/link-bins'
import pick from 'ramda/src/pick'
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
    description: 'Updates pnpm to the latest version',
    descriptionLists: [],
    url: docsUrl('self-update'),
    usages: [],
  })
}

export type SelfUpdateCommandOptions = InstallCommandOptions

export async function handler (
  opts: SelfUpdateCommandOptions
): Promise<undefined | string> {
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_SELF_UPDATE_IN_COREPACK', 'You should update pnpm with corepack')
  }
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  const pkgName = 'pnpm'
  const resolution = await resolve({ alias: pkgName, pref: 'latest' }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: pickRegistryForPackage(opts.registries, pkgName, 'latest'),
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', 'Cannot find latest version of pnpm')
  }
  if (resolution.manifest.version === packageManager.version) {
    return `The currently active ${packageManager.name} v${packageManager.version} is already "latest" and doesn't need an update`
  }

  const currentPkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: resolution.manifest.version,
    },
  })
  if (fs.existsSync(dir)) {
    await linkBins(path.join(dir, opts.modulesDir ?? 'node_modules'), opts.pnpmHomeDir,
      {
        warn: globalWarn,
      }
    )
    return `The latest version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${dir}.`
  }
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), '{}')
  await add.handler(
    {
      ...opts,
      dir,
      lockfileDir: dir,
      bin: opts.pnpmHomeDir,
    },
    [`${currentPkgName}@${resolution.manifest.version}`]
  )
  return undefined
}
