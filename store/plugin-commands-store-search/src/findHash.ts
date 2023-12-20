import path from 'path'
import fs from 'fs'
import chalk from 'chalk'

import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { getStorePath } from '@pnpm/store-path'

import renderHelp from 'render-help'

const PACKAGE_INFO_CLR = chalk.greenBright
const INDEX_PATH_CLR = chalk.hex('#078487')

export const commandNames = ['find-hash']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description:
      'Print the contained packages according to the hash.',
    descriptionLists: [],
    usages: ['pnpm find-hash <hash>'],
  })
}

export type findHashCommandOptions = Pick<Config, 'storeDir' | 'pnpmHomeDir'>
export interface findHashResult {
  name: string
  version: string
  indexPath: string
}

export async function handler (opts: findHashCommandOptions, params: string[]) {
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
  const cafsChildrenDirs = fs.readdirSync(cafsDir).filter(dirName => fs.statSync(`${cafsDir}/${dirName}`).isDirectory())
  const indexFiles: string[] = []; const result: findHashResult[] = []

  cafsChildrenDirs.forEach((dirName) => {
    const dirIndexFiles = fs
      .readdirSync(`${cafsDir}/${dirName}`)
      .filter((fileName) => fileName.includes('-index.json'))
      ?.map((fileName) => `${cafsDir}/${dirName}/${fileName}`)

    indexFiles.push(...dirIndexFiles)
  })

  indexFiles.forEach(indexPath => {
    const data = JSON.parse(fs.readFileSync(indexPath, 'utf-8') || '{}')
    if (!data?.name || !data?.files) return
    Object.keys(data.files).forEach(key => {
      if (data.files?.[key]?.integrity === hash) {
        result.push({ name: data.name, version: data?.version || 'latest', indexPath: indexPath.replace(cafsDir, '') })
      }
    })
  })

  if (!result.length) {
    throw new PnpmError(
      'INVALID_FILE_HASH',
      'No package or index file matching this hash was found.'
    )
  }

  return result.reduce((acc, { name, version, indexPath }) => {
    acc += `${PACKAGE_INFO_CLR(name)}@${PACKAGE_INFO_CLR(version)}  ${INDEX_PATH_CLR(indexPath)}\n`; return acc
  }, '')
}
