import path from 'node:path'

const SEP = path.sep

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
  return `${storeDir}${SEP}${contentPathFromHex(fileType, hexDigest)}`
}

export function contentPathFromHex (fileType: FileType, hex: string): string {
  // Using template strings with path.sep instead of path.join() for performance.
  // This is a hot path called ~30k times per cold install; avoiding path.join
  // saves ~30ms per install by eliminating per-call argument validation overhead.
  const p = `files${SEP}${hex.slice(0, 2)}${SEP}${hex.slice(2)}`
  switch (fileType) {
    case 'exec':
      return `${p}-exec`
    case 'nonexec':
      return p
  }
}
