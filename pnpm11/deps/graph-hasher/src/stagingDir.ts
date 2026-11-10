import { randomBytes } from 'node:crypto'
import path from 'node:path'

const STAGING_DIR_PREFIX = '.pnpm-'

/**
 * A token that names one install's staging directories: 8 hex characters of
 * fresh randomness. It keeps installs apart, including installs in different
 * containers or on different hosts that share a store, and stays short so a
 * build's working directory fits within the Windows path length limit.
 */
export function createStagingToken (): string {
  return randomBytes(4).toString('hex')
}

/**
 * The staging directory an install builds a global virtual store package in:
 * `.pnpm-<token>` beside the package directory, so a scoped package stages in
 * its scope directory. A directory an install moves aside while placing the
 * package keeps this name and adds a suffix.
 */
export function getStagingDir (pkgDir: string, token: string): string {
  return path.join(path.dirname(pkgDir), `${STAGING_DIR_PREFIX}${token}`)
}

/** Whether `entryName`, beside a package directory, is a staging directory or one moved aside from placement. */
export function isStagingDirName (entryName: string): boolean {
  return entryName.startsWith(STAGING_DIR_PREFIX)
}
