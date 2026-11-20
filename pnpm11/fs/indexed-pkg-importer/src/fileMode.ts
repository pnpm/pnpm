import path from 'node:path'

const BASE_FILE_MODE = 0o666
const EXEC_FILE_MODE = 0o755
const CAFS_EXEC_SUFFIX = '-exec'
const CAFS_DIGEST_MIN_LENGTH = 40

// The CLI never changes its own umask and the import path asks for it once
// per file, so read it once.
let umask: number | undefined

// A store file carries the umask that was active when it was written, so
// importing has to apply the current one as though the entry were just
// written (pnpm/pnpm#3807). POSIX mode bits do not apply on Windows.
export function storeEntryModeForSource (src: string): number | undefined {
  if (process.platform === 'win32' || !isCafsFile(src)) return undefined
  umask ??= process.umask()
  return ((src.endsWith(CAFS_EXEC_SUFFIX) ? EXEC_FILE_MODE : BASE_FILE_MODE) & ~umask) & 0o777
}

// The mode an import of `src` materializes: the store-entry mode for a store
// file, otherwise the source's own mode, which a local-directory dependency keeps.
export function fileModeForImport (sourceMode: bigint, storeMode: number | undefined): number | undefined {
  if (process.platform === 'win32') return undefined
  return storeMode ?? Number(sourceMode & 0o777n)
}

// Group- and other-write may be missing, and group-write may be extra: a
// store write takes those bits from its shard directory, not from the umask,
// and the shared inode already grants them through the store. Every other bit
// must match, so a store populated under a wider umask is still not linked.
export function isLinkableStoreMode (mode: number, storeMode: number): boolean {
  const groupOtherWrite = 0o022
  return ((mode ^ storeMode) & 0o777 & ~groupOtherWrite) === 0 && (mode & ~storeMode & 0o002) === 0
}

// The exact inverse of the CAFS layout `<store>/v<N>/files/<2 hex digits>/<digest>[-exec]`.
// This importer also materializes local-directory dependencies, whose files keep
// the modes their project gives them, so only a real store entry is re-derived.
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
