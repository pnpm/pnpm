import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { FULL_FILTERED_META_DIR, ABBREVIATED_META_DIR } from '@pnpm/constants'
import { getStorePath } from '@pnpm/store-path'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import {
  cacheList,
  cacheView,
  cacheDelete,
  cacheListRegistries,
} from '@pnpm/cache.api'
import { PnpmError } from '@pnpm/error'

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
  const cacheType = (opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField)
    ? FULL_FILTERED_META_DIR
    : ABBREVIATED_META_DIR
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
    if (!params[1]) {
      throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm cache view` requires the package name')
    }
    if (params.length > 2) {
      throw new PnpmError('TOO_MANY_PARAMS', '`pnpm cache view` only accepts one package name')
    }
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
