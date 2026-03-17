import util from 'node:util'

import { createResolver } from '@pnpm/client'
import type { Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import type { TarballResolution } from '@pnpm/lockfile.types'
import { sortDeepKeys } from '@pnpm/object.key-sorting'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { getStorePath } from '@pnpm/store-path'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { renderHelp } from 'render-help'

export const skipPackageManagerCheck = true

export const commandNames = ['cat-index']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function help (): string {
  return renderHelp({
    description: 'Prints the index file of a specific package from the store.',
    descriptionLists: [],
    usages: ['pnpm cat-index <pkg name>@<pkg version>'],
  })
}

export type CatIndexCommandOptions = Pick<
  Config,
| 'rawConfig'
| 'pnpmHomeDir'
| 'storeDir'
| 'lockfileDir'
| 'dir'
| 'registries'
| 'cacheDir'
| 'sslConfigs'
>

export async function handler (opts: CatIndexCommandOptions, params: string[]): Promise<string> {
  if (!params || params.length === 0) {
    throw new PnpmError(
      'MISSING_PACKAGE_NAME',
      'Specify a package',
      {
        hint: help(),
      }
    )
  }

  const wantedDependency = params[0]
  const { alias, bareSpecifier } = parseWantedDependency(wantedDependency) || {}

  if (!alias) {
    throw new PnpmError(
      'INVALID_SELECTOR',
      `Cannot parse the "${wantedDependency}" selector`
    )
  }

  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
  })
  const pkgSnapshot = await resolve(
    { alias, bareSpecifier },
    {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      preferredVersions: {},
      projectDir: opts.dir,
    }
  )

  const filesIndexFile = storeIndexKey(
    (pkgSnapshot.resolution as TarballResolution).integrity!.toString(),
    `${alias}@${bareSpecifier}`
  )
  const storeIndex = new StoreIndex(storeDir)
  try {
    const pkgFilesIndex = storeIndex.get(filesIndexFile) as PackageFilesIndex | undefined
    if (!pkgFilesIndex) {
      throw new PnpmError(
        'INVALID_PACKAGE',
        'No corresponding index file found. You can use pnpm list to see if the package is installed.'
      )
    }
    return JSON.stringify(sortDeepKeys(pkgFilesIndex), replacer, 2)
  } finally {
    storeIndex.close()
  }
}

function replacer (key: string, value: unknown) {
  if (util.types.isMap(value)) {
    const entries = Array.from((value as Map<string, unknown>).entries())
    entries.sort(([key1], [key2]) => lexCompare(key1, key2))
    return Object.fromEntries(entries)
  }
  return value
}
