import path from 'path'
import ssri, { type IntegrityLike } from 'ssri'

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

export type FileType = 'exec' | 'nonexec' | 'index'

export function getFilePathByModeInCafs (
  storeDir: string,
  integrity: string | IntegrityLike,
  mode: number
): string {
  const fileType = modeIsExecutable(mode) ? 'exec' : 'nonexec'
  return path.join(storeDir, contentPathFromIntegrity(integrity, fileType))
}

export function getIndexFilePathInCafs (
  storeDir: string,
  integrity: string | IntegrityLike,
  pkgId: string
): string {
  const hex = ssri.parse(integrity, { single: true }).hexDigest().substring(0, 64)
  // Some registries allow identical content to be published under different package names or versions.
  // To accommodate this, index files are stored using both the content hash and package identifier.
  // This approach ensures that we can:
  // 1. Validate that the integrity in the lockfile corresponds to the correct package,
  //    which might not be the case after a poorly resolved Git conflict.
  // 2. Allow the same content to be referenced by different packages or different versions of the same package.
  return path.join(storeDir, `index/${path.join(hex.slice(0, 2), hex.slice(2))}-${pkgId.replace(/[\\/:*?"<>|]/g, '+')}.json`)
}

function contentPathFromIntegrity (
  integrity: string | IntegrityLike,
  fileType: FileType
): string {
  const sri = ssri.parse(integrity, { single: true })
  return contentPathFromHex(fileType, sri.hexDigest())
}

export function contentPathFromHex (fileType: FileType, hex: string): string {
  const p = path.join('files', hex.slice(0, 2), hex.slice(2))
  switch (fileType) {
  case 'exec':
    return `${p}-exec`
  case 'nonexec':
    return p
  case 'index':
    return `${p}-index.json`
  }
}
