import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'
import { getStorePath } from '@pnpm/store.path'
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
  const hash = parseHashParam(params)
  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const result = scanStoreForHash(storeDir, hash)
  if (!result.length) {
    throw new PnpmError(
      'INVALID_FILE_HASH',
      'No package or index file matching this hash was found.'
    )
  }
  return formatResults(result)
}

function parseHashParam (params: string[]): string {
  if (!params || params.length === 0) {
    throw new PnpmError('MISSING_HASH', '`pnpm find-hash` requires the hash')
  }
  let hash = params[0]
  if (hash.includes('-')) {
    const base64Part = hash.split('-').slice(1).join('-')
    hash = Buffer.from(base64Part, 'base64').toString('hex')
  }
  return hash.toLowerCase()
}

function scanStoreForHash (storeDir: string, hash: string): FindHashResult[] {
  const result: FindHashResult[] = []
  const storeIndex = new StoreIndex(storeDir)
  try {
    for (const [indexKey, data] of storeIndex.entries()) {
      const pkgFilesIndex = data as PackageFilesIndex
      if (!pkgFilesIndex) continue
      if (pkgFilesIndexContainsHash(pkgFilesIndex, hash)) {
        result.push({
          name: pkgFilesIndex.manifest?.name ?? 'unknown',
          version: pkgFilesIndex.manifest?.version ?? 'unknown',
          indexKey,
        })
      }
    }
  } finally {
    storeIndex.close()
  }
  return result
}

function pkgFilesIndexContainsHash (pkgFilesIndex: PackageFilesIndex, hash: string): boolean {
  return filesContainHash(pkgFilesIndex.files, hash) || sideEffectsContainHash(pkgFilesIndex.sideEffects, hash)
}

function filesContainHash (files: PackageFilesIndex['files'], hash: string): boolean {
  if (!files) return false
  for (const file of files.values()) {
    if (file?.digest === hash) {
      return true
    }
  }
  return false
}

function sideEffectsContainHash (sideEffects: PackageFilesIndex['sideEffects'], hash: string): boolean {
  if (!sideEffects) return false
  for (const { added } of sideEffects.values()) {
    if (added && filesContainHash(added, hash)) {
      return true
    }
  }
  return false
}

function formatResults (result: FindHashResult[]): string {
  let acc = ''
  for (const { name, version, indexKey } of result) {
    acc += `${PACKAGE_INFO_CLR(name)}@${PACKAGE_INFO_CLR(version)}  ${INDEX_PATH_CLR(indexKey)}\n`
  }
  return acc
}
