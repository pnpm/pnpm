import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { cacheListCmd } from './cacheList.cmd'
import { cacheDeleteCmd } from './cacheDelete.cmd'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'registry',
      'store-dir',
    ], allTypes),
    registries: Boolean,
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

export type CacheCommandOptions = {
  registries?: boolean
} & Pick<Config, 'cacheDir' | 'cliOptions'>

export async function handler (opts: CacheCommandOptions, params: string[]): Promise<string | undefined> {
  switch (params[0]) {
  case 'list':
    return cacheListCmd({
      ...opts,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  case 'delete':
    return cacheDeleteCmd({
      ...opts,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  default:
    return help()
  }
}
