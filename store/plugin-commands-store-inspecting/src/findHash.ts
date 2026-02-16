import path from 'path'
import fs from 'fs'
import chalk from 'chalk'

import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { readMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { getStorePath } from '@pnpm/store-path'
import { type PackageFilesIndex } from '@pnpm/store.cafs'

import renderHelp from 'render-help'

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
  filesIndexFile: string
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
  const indexDir = path.join(storeDir, 'index')
  const cafsChildrenDirs = fs.readdirSync(indexDir, { withFileTypes: true }).filter(file => file.isDirectory())
  const indexFiles: string[] = []; const result: FindHashResult[] = []

  for (const { name: dirName } of cafsChildrenDirs) {
    const dirIndexFiles = fs
      .readdirSync(`${indexDir}/${dirName}`)
      .filter((fileName) => fileName.includes('.mpk'))
      ?.map((fileName) => `${indexDir}/${dirName}/${fileName}`)

    indexFiles.push(...dirIndexFiles)
  }

  for (const filesIndexFile of indexFiles) {
    let pkgFilesIndex: PackageFilesIndex | undefined
    try {
      pkgFilesIndex = readMsgpackFileSync<PackageFilesIndex>(filesIndexFile)
    } catch {
      continue
    }
    if (!pkgFilesIndex) continue

    if (pkgFilesIndex.files) {
      for (const file of pkgFilesIndex.files.values()) {
        if (file?.digest === hash) {
          result.push({ name: pkgFilesIndex.manifest?.name ?? 'unknown', version: pkgFilesIndex.manifest?.version ?? 'unknown', filesIndexFile: filesIndexFile.replace(indexDir, '') })

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
            result.push({ name: pkgFilesIndex.manifest?.name ?? 'unknown', version: pkgFilesIndex.manifest?.version ?? 'unknown', filesIndexFile: filesIndexFile.replace(indexDir, '') })

            // a package is only found once.
            continue
          }
        }
      }
    }
  }

  if (!result.length) {
    throw new PnpmError(
      'INVALID_FILE_HASH',
      'No package or index file matching this hash was found.'
    )
  }

  let acc = ''
  for (const { name, version, filesIndexFile } of result) {
    acc += `${PACKAGE_INFO_CLR(name)}@${PACKAGE_INFO_CLR(version)}  ${INDEX_PATH_CLR(filesIndexFile)}\n`
  }
  return acc
}
