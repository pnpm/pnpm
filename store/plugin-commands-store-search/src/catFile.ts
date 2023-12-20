import path from 'path'

import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import gfs from '@pnpm/graceful-fs'
import { getStorePath } from '@pnpm/store-path'

import renderHelp from 'render-help'

const INTEGRITY_REGEX: RegExp = /^([^-]+)-([A-Za-z0-9+/=]+)$/

export const commandNames = ['cat-file']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description:
      'Prints the contents of a file based on the hash value stored in the index file.',
    descriptionLists: [],
    usages: ['pnpm cat-file <hash>'],
  })
}

export type catFileCommandOptions = Pick<Config, 'storeDir' | 'pnpmHomeDir'>

export async function handler (opts: catFileCommandOptions, params: string[]) {
  if (!params || params.length === 0) {
    throw new PnpmError('MISSING_HASH', '`pnpm cat-file` requires the hash')
  }

  const [, , integrityHash] = params[0].match(INTEGRITY_REGEX)!

  const toHex = Buffer.from(integrityHash, 'base64').toString('hex')
  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const cafsDir = path.join(storeDir, 'files')
  const filePath = path.resolve(cafsDir, toHex.slice(0, 2), toHex.slice(2))

  try {
    const fileContent = await gfs.readFile(filePath, 'utf8')

    logger.info({
      message: fileContent,
      prefix: process.cwd(),
    })
  } catch {
    logger.error(
      new PnpmError('INVALID_HASH', 'Corresponding hash file not found')
    )
  }
}
