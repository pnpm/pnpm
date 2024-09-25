import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { FULL_FILTERED_META_DIR, META_DIR } from '@pnpm/constants'
import { getStorePath } from '@pnpm/store-path'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import {
  cacheList,
  cacheView,
  cacheDelete,
  cacheListRegistries,
} from '@pnpm/cache.api'

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
    description: 'Inspect and manage the metadata cache',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'Lists the available packages metadata cache. Supports filtering by glob',
            name: 'list',
          },
          {
            description: 'Lists all registries that have their metadata cache locally',
            name: 'list-registries',
          },
          {
            description: "Views information from the specified package's cache",
            name: 'view',
          },
          {
            description: 'Deletes metadata cache for the specified package(s). Supports patterns',
            name: 'delete',
          },
        ],
      },
    ],
    url: docsUrl('cache'),
    usages: ['pnpm cache <command>'],
  })
}

export type CacheCommandOptions = Pick<Config, 'cacheDir' | 'storeDir' | 'pnpmHomeDir' | 'cliOptions' | 'resolutionMode' | 'registrySupportsTimeField'>

export async function handler (opts: CacheCommandOptions, params: string[]): Promise<string | undefined> {
  const cacheType = opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField ? FULL_FILTERED_META_DIR : META_DIR
  const cacheDir = path.join(opts.cacheDir, cacheType)
  switch (params[0]) {
  case 'list-registries':
    return cacheListRegistries({
      ...opts,
      cacheDir,
    })
  case 'list':
    return cacheList({
      ...opts,
      cacheDir,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  case 'delete':
    return cacheDelete({
      ...opts,
      cacheDir,
      registry: opts.cliOptions['registry'],
    }, params.slice(1))
  case 'view': {
    const storeDir = await getStorePath({
      pkgRoot: process.cwd(),
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })
    return cacheView({
      ...opts,
      cacheDir,
      storeDir,
      registry: opts.cliOptions['registry'],
    }, params[1])
  }
  default:
    return help()
  }
}
