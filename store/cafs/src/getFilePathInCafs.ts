import path from 'path'
import { PnpmError } from '@pnpm/error'

/**
 * Checks if a file mode has any executable permissions set.
 *
 * This function performs a bitwise check to determine if at least one of the
 * executable bits (owner, group, or others) is set in the file mode.
 *
 * The bit mask `0o111` corresponds to the executable bits for the owner (0o100),
 * group (0o010), and others (0o001). If any of these bits are set, the file
 * is considered executable.
 *
 * @param {number} mode - The file mode (permission bits) to check.
 * @returns {boolean} - Returns true if any of the executable bits are set, false otherwise.
 */
export const modeIsExecutable = (mode: number): boolean => (mode & 0o111) !== 0

export type FileType = 'exec' | 'nonexec'

export function getFilePathByModeInCafs (
  storeDir: string,
  hexDigest: string,
  mode: number
): string {
  const fileType = modeIsExecutable(mode) ? 'exec' : 'nonexec'
  return path.join(storeDir, contentPathFromHex(fileType, hexDigest))
}

export function getIndexFilePathInCafs (
  storeDir: string,
  integrity: string,
  pkgId: string
): string {
  // integrity is in format "algo-base64hash", extract and convert the base64 part to hex
  const dashIndex = integrity.indexOf('-')
  if (dashIndex === -1) {
    throw new PnpmError('INVALID_INTEGRITY', `Invalid integrity format: expected "algo-base64hash", got "${integrity}"`)
  }
  const base64Part = integrity.slice(dashIndex + 1)
  const hex = Buffer.from(base64Part, 'base64').toString('hex').substring(0, 64)
  // Some registries allow identical content to be published under different package names or versions.
  // To accommodate this, index files are stored using both the content hash and package identifier.
  // This approach ensures that we can:
  // 1. Validate that the integrity in the lockfile corresponds to the correct package,
  //    which might not be the case after a poorly resolved Git conflict.
  // 2. Allow the same content to be referenced by different packages or different versions of the same package.
  return path.join(storeDir, `index/${path.join(hex.slice(0, 2), hex.slice(2))}-${pkgId.replace(/[\\/:*?"<>|]/g, '+')}.mpk`)
}

export function contentPathFromHex (fileType: FileType, hex: string): string {
  const p = path.join('files', hex.slice(0, 2), hex.slice(2))
  switch (fileType) {
  case 'exec':
    return `${p}-exec`
  case 'nonexec':
    return p
  }
}
