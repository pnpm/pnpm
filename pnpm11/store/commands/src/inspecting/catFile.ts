import path from 'node:path'

import type { Config } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import { getFilePathByModeInCafs } from '@pnpm/store.cafs'
import { getStorePath } from '@pnpm/store.path'
import { renderHelp } from 'render-help'

const INTEGRITY_REGEX: RegExp = /^[^-]+-([a-z0-9+/=]+)$/i

export const skipPackageManagerCheck = true

export const commandNames = ['cat-file']

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function help (): string {
  return renderHelp({
    description:
      'Prints the contents of a file based on the hash value stored in the index file.',
    descriptionLists: [],
    usages: ['pnpm cat-file <hash>'],
  })
}

export type CatFileCommandOptions = Pick<Config, 'storeDir' | 'pnpmHomeDir'>

export async function handler (opts: CatFileCommandOptions, params: string[]): Promise<string> {
  if (!params || params.length === 0) {
    throw new PnpmError('MISSING_HASH', 'Missing file hash', {
      hint: help(),
    })
  }

  const match = INTEGRITY_REGEX.exec(params[0])
  if (!match) {
    throw new PnpmError('INVALID_HASH', `Invalid hash format. Expected something like sha512-..., got ${params[0]}`)
  }

  const toHex = Buffer.from(match[1], 'base64').toString('hex')
  if (toHex.length <= 2) {
    throw new PnpmError('INVALID_HASH', `Invalid hash format. Expected something like sha512-..., got ${params[0]}`)
  }

  const storeDir = await getStorePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const filePath = path.resolve(getFilePathByModeInCafs(storeDir, toHex, 0))

  try {
    return gfs.readFileSync(filePath, 'utf8')
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new PnpmError('INVALID_HASH', 'Corresponding hash file not found')
    }
    throw err
  }
}
