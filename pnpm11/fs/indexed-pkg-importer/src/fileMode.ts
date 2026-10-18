import path from 'node:path'

const BASE_FILE_MODE = 0o666
const EXEC_FILE_MODE = 0o755
const CAFS_EXEC_SUFFIX = '-exec'
const CAFS_DIGEST_MIN_LENGTH = 40

let umask: number | undefined

export function storeEntryModeForSource (src: string): number | undefined {
  if (process.platform === 'win32' || !isCafsFile(src)) return undefined
  umask ??= process.umask()
  return ((src.endsWith(CAFS_EXEC_SUFFIX) ? EXEC_FILE_MODE : BASE_FILE_MODE) & ~umask) & 0o777
}

export function fileModeForImport (src: string, sourceMode: bigint): number | undefined {
  if (process.platform === 'win32') return undefined
  return storeEntryModeForSource(src) ?? Number(sourceMode & 0o777n)
}

function isCafsFile (src: string): boolean {
  const nameSep = src.lastIndexOf(path.sep)
  if (nameSep < 0) return false
  const shardSep = src.lastIndexOf(path.sep, nameSep - 1)
  if (shardSep < 0 || nameSep - shardSep - 1 !== 2 || !isLowerHex(src, shardSep + 1, nameSep)) return false
  const filesSep = src.lastIndexOf(path.sep, shardSep - 1)
  if (filesSep < 0 || shardSep - filesSep - 1 !== 'files'.length || !src.startsWith('files', filesSep + 1)) return false
  const digestEnd = src.endsWith(CAFS_EXEC_SUFFIX) ? src.length - CAFS_EXEC_SUFFIX.length : src.length
  return digestEnd - nameSep - 1 >= CAFS_DIGEST_MIN_LENGTH && isLowerHex(src, nameSep + 1, digestEnd)
}

function isLowerHex (value: string, from: number, to: number): boolean {
  for (let index = from; index < to; index++) {
    const code = value.charCodeAt(index)
    if (!((code >= 0x30 && code <= 0x39) || (code >= 0x61 && code <= 0x66))) return false
  }
  return true
}
