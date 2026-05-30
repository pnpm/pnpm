import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

// Mirrors the constant in
// `installing/deps-installer/src/install/verifyLockfileResolutionsCache.ts`.
// Kept in sync by hand (both sides own a small string; introducing a
// shared package just for this would outweigh the cost of the duplicate).
const LOCKFILE_VERIFIED_CACHE_FILE = 'lockfile-verified.jsonl'

/**
 * Remove the lockfile-verification cache JSONL written by the install
 * command's resolution-policy verifier. Pruning the store invalidates
 * derived state; a stale verification record under a different
 * policy/lockfile-content key would otherwise survive into the next
 * install (still correct because of the cache's identity comparator,
 * but visually leaks "alien" files into `cacheDir`).
 *
 * Silent on a missing file — prune is idempotent and the cache may
 * never have been written in the first place.
 */
export function cleanLockfileVerifiedCache (cacheDir: string): void {
  const cacheFilePath = path.join(cacheDir, LOCKFILE_VERIFIED_CACHE_FILE)
  try {
    fs.unlinkSync(cacheFilePath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
}
