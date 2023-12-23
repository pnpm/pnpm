import path from 'path'

import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
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

export type CatFileCommandOptions = Pick<Config, 'storeDir' | 'pnpmHomeDir'>

export async function handler (opts: CatFileCommandOptions, params: string[]) {
  if (!params || params.length === 0) {
    throw new PnpmError('MISSING_HASH', 'Missing file hash', {
      hint: help(),
    })
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
    return gfs.readFileSync(filePath, 'utf8')
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') {
      throw new PnpmError('INVALID_HASH', 'Corresponding hash file not found')
    }
    throw err
  }
}
