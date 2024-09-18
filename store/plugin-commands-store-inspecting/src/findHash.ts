import path from 'path'
import fs from 'fs'
import chalk from 'chalk'

import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { getStorePath } from '@pnpm/store-path'
import { type PackageFilesIndex } from '@pnpm/store.cafs'

import loadJsonFile from 'load-json-file'
import renderHelp from 'render-help'

export const PACKAGE_INFO_CLR = chalk.greenBright
export const INDEX_PATH_CLR = chalk.hex('#078487')

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

  const hash = params[0]
  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const cafsDir = path.join(storeDir, 'files')
  const cafsChildrenDirs = fs.readdirSync(cafsDir, { withFileTypes: true }).filter(file => file.isDirectory())
  const indexFiles: string[] = []; const result: FindHashResult[] = []

  for (const { name: dirName } of cafsChildrenDirs) {
    const dirIndexFiles = fs
      .readdirSync(`${cafsDir}/${dirName}`)
      .filter((fileName) => fileName.includes('-index.json'))
      ?.map((fileName) => `${cafsDir}/${dirName}/${fileName}`)

    indexFiles.push(...dirIndexFiles)
  }

  for (const filesIndexFile of indexFiles) {
    const pkgFilesIndex = loadJsonFile.sync<PackageFilesIndex>(filesIndexFile)

    for (const [, file] of Object.entries(pkgFilesIndex.files)) {
      if (file?.integrity === hash) {
        result.push({ name: pkgFilesIndex.name ?? 'unknown', version: pkgFilesIndex?.version ?? 'unknown', filesIndexFile: filesIndexFile.replace(cafsDir, '') })

        // a package is only found once.
        continue
      }
    }

    if (pkgFilesIndex?.sideEffects) {
      for (const [, files] of Object.entries(pkgFilesIndex.sideEffects)) {
        for (const [, file] of Object.entries(files)) {
          if (file?.integrity === hash) {
            result.push({ name: pkgFilesIndex.name ?? 'unknown', version: pkgFilesIndex?.version ?? 'unknown', filesIndexFile: filesIndexFile.replace(cafsDir, '') })

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
