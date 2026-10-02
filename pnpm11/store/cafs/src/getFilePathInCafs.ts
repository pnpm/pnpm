import path from 'node:path'

const SEP = path.sep

/**
 * Checks if a file mode has any executable permissions set.
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
  const contentPath = `files${SEP}${hex.slice(0, 2)}${SEP}${hex.slice(2)}`
  switch (fileType) {
    case 'exec':
      return `${contentPath}-exec`
    case 'nonexec':
      return contentPath
  }
}
