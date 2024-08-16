import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { getCurrentPackageName, packageManager } from '@pnpm/cli-meta'
import { createResolver } from '@pnpm/client'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { getToolDirPath } from '@pnpm/tools.path'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { type InstallCommandOptions } from './install'
import * as add from './add'

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
    description: '',
    descriptionLists: [],
    url: docsUrl('self-update'),
    usages: [],
  })
}

export type SelfUpdateCommandOptions = InstallCommandOptions

export async function handler (
  opts: SelfUpdateCommandOptions,
  params: string[]
): Promise<void> {
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
  if (resolution.manifest.version === packageManager.version) return

  const currentPkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: resolution.manifest.version,
    },
  })
  if (fs.existsSync(dir)) return
  const version = resolution.manifest.version

  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), '{}')
  await add.handler(
    {
      ...opts,
      dir,
      lockfileDir: dir,
      bin: opts.pnpmHomeDir,
    },
    [`${currentPkgName}@${version}`]
  )
}

