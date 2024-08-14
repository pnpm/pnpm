import { docsUrl } from '@pnpm/cli-utils'
import {
  createResolver,
} from '@pnpm/client'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { prepareExecutionEnv } from '@pnpm/plugin-commands-env'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { type InstallCommandOptions } from './install'
import { installDeps } from './installDeps'

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
    lockfileDir: opts.lockfileDir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: pickRegistryForPackage(opts.registries, pkgName, 'latest'),
  })
  const version = resolution.manifest.version

  return resolution?.manifest ?? null
}

