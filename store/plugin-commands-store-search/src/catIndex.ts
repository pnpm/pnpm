import path from 'path'

import { type Config } from '@pnpm/config'
import { createResolver, type ClientOptions } from '@pnpm/client'
import { type TarballResolution } from '@pnpm/lockfile-types'

import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import { getStorePath } from '@pnpm/store-path'
import { getFilePathInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'

import loadJsonFile from 'load-json-file'
import renderHelp from 'render-help'

export const commandNames = ['cat-index']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description:
      'Prints the corresponding index file based on package name and version.',
    descriptionLists: [],
    usages: ['pnpm cat-index <pkg name>@<pkg version>'],
  })
}

export type catIndexCommandOptions = Pick<
Config,
| 'rawConfig'
| 'pnpmHomeDir'
| 'storeDir'
| 'lockfileDir'
| 'dir'
| 'registries'
> &
ClientOptions

export async function handler (opts: catIndexCommandOptions, params: string[]) {
  if (!params || params.length === 0) {
    throw new PnpmError(
      'MISSING_PACKAGE_NAME',
      '`pnpm cat-index` requires the package name'
    )
  }

  const wantedDependency = params[0]
  const { alias, pref } = parseWantedDependency(wantedDependency) || {}

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
  const cafsDir = path.join(storeDir, 'files')
  const resolve = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
  })
  const pkgSnapshot = await resolve(
    { alias, pref },
    {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      preferredVersions: {},
      projectDir: opts.dir,
      registry: pickRegistryForPackage(opts.registries, alias, pref),
    }
  )

  const filesIndexFile = getFilePathInCafs(
    cafsDir,
    (pkgSnapshot.resolution as TarballResolution).integrity!.toString(),
    'index'
  )

  try {
    const pkgFilesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
    console.log(pkgFilesIndex)
  } catch {
    logger.error(
      new PnpmError(
        'INVALID_PACKAGE',
        'No corresponding index file found. You can use pnpm list to see if the package is installed.'
      )
    )
  }
}
