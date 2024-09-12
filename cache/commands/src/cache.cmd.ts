import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { FULL_FILTERED_META_DIR, META_DIR } from '@pnpm/constants'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { cacheListCmd, cacheListRegistriesCmd } from './cacheList.cmd'
import { cacheDeleteCmd } from './cacheDelete.cmd'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'registry',
      'store-dir',
    ], allTypes),
  }
}

export const commandNames = ['cache']

export function help (): string {
  return renderHelp({
    description: '',
    descriptionLists: [],
    url: docsUrl('cache'),
    usages: ['pnpm cache <command>'],
  })
}

export type CacheCommandOptions = Pick<Config, 'cacheDir' | 'cliOptions' | 'resolutionMode' | 'registrySupportsTimeField'>

export async function handler (opts: CacheCommandOptions, params: string[]): Promise<string | undefined> {
  const cacheType = opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField ? FULL_FILTERED_META_DIR : META_DIR
  const cacheDir = path.join(opts.cacheDir, cacheType)
  switch (params[0]) {
  case 'list-registries':
    return cacheListRegistriesCmd({
      ...opts,
      cacheDir,
    })
  case 'list':
    return cacheListCmd({
      ...opts,
      cacheDir,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  case 'delete':
    return cacheDeleteCmd({
      ...opts,
      cacheDir,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  default:
    return help()
  }
}
