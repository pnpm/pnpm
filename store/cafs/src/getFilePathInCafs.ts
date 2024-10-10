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
  cafsDir: string,
  integrity: string | IntegrityLike,
  mode: number
): string {
  const fileType = modeIsExecutable(mode) ? 'exec' : 'nonexec'
  return path.join(cafsDir, contentPathFromIntegrity(integrity, fileType))
}

export function getIndexFilePathInCafs (
  cafsDir: string,
  integrity: string | IntegrityLike
): string {
  return path.join(cafsDir, contentPathFromIntegrity(integrity, 'index'))
}

function contentPathFromIntegrity (
  integrity: string | IntegrityLike,
  fileType: FileType
): string {
  const sri = ssri.parse(integrity, { single: true })
  return contentPathFromHex(fileType, sri.hexDigest())
}

export function contentPathFromHex (fileType: FileType, hex: string): string {
  const p = path.join(hex.slice(0, 2), hex.slice(2))
  switch (fileType) {
  case 'exec':
    return `${p}-exec`
  case 'nonexec':
    return p
  case 'index':
    return `${p}-index.json`
  }
}
