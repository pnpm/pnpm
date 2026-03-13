import type { Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'
import { getStorePath } from '@pnpm/store-path'
import chalk from 'chalk'
import { renderHelp } from 'render-help'

export const PACKAGE_INFO_CLR = chalk.greenBright
export const INDEX_PATH_CLR = chalk.hex('#078487')

export const skipPackageManagerCheck = true

export const commandNames = ['find-hash']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function help (): string {
  return renderHelp({
    description:
      'Experimental! Lists the packages that include the file with the specified hash.',
    descriptionLists: [],
    usages: ['pnpm find-hash <hash>'],
  })
}

export type FindHashCommandOptions = Pick<Config, 'storeDir' | 'pnpmHomeDir'>
export interface FindHashResult {
  name: string
  version: string
  indexKey: string
}

export async function handler (opts: FindHashCommandOptions, params: string[]): Promise<string> {
  if (!params || params.length === 0) {
    throw new PnpmError('MISSING_HASH', '`pnpm find-hash` requires the hash')
  }

  // Convert the input hash to hex format for comparison
  // Input can be either:
  // - A hex string (used directly)
  // - A base64 integrity string like "sha512-..." (converted to hex)
  let hash = params[0]
  if (hash.includes('-')) {
    // Looks like an integrity string (algo-base64), extract and convert the base64 part
    const base64Part = hash.split('-').slice(1).join('-')
    hash = Buffer.from(base64Part, 'base64').toString('hex')
  }
  // Stored digests are lowercase hex, so normalize the input to lowercase
  hash = hash.toLowerCase()
  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const result: FindHashResult[] = []
  const storeIndex = new StoreIndex(storeDir)

  try {
    for (const [indexKey, data] of storeIndex.entries()) {
      const pkgFilesIndex = data as PackageFilesIndex
      if (!pkgFilesIndex) continue

      if (pkgFilesIndex.files) {
        for (const file of pkgFilesIndex.files.values()) {
          if (file?.digest === hash) {
            result.push({ name: pkgFilesIndex.manifest?.name ?? 'unknown', version: pkgFilesIndex.manifest?.version ?? 'unknown', indexKey })

            // a package is only found once.
            continue
          }
        }
      }

      if (pkgFilesIndex.sideEffects) {
        for (const { added } of pkgFilesIndex.sideEffects.values()) {
          if (!added) continue
          for (const file of added.values()) {
            if (file?.digest === hash) {
              result.push({ name: pkgFilesIndex.manifest?.name ?? 'unknown', version: pkgFilesIndex.manifest?.version ?? 'unknown', indexKey })

              // a package is only found once.
              continue
            }
          }
        }
      }
    }
  } finally {
    storeIndex.close()
  }

  if (!result.length) {
    throw new PnpmError(
      'INVALID_FILE_HASH',
      'No package or index file matching this hash was found.'
    )
  }

  let acc = ''
  for (const { name, version, indexKey } of result) {
    acc += `${PACKAGE_INFO_CLR(name)}@${PACKAGE_INFO_CLR(version)}  ${INDEX_PATH_CLR(indexKey)}\n`
  }
  return acc
}
