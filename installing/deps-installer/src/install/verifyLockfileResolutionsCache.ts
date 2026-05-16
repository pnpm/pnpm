import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { logger } from '@pnpm/logger'

/**
 * On-disk cache of verifyLockfileResolutions results, keyed by absolute
 * lockfile path. Lets repeat installs against an unchanged lockfile skip
 * the per-package registry round trips entirely.
 *
 * Persisted as JSON Lines: each verification appends one record, the latest
 * record per path wins on read. Appends of a single line are atomic on
 * POSIX and NTFS, so parallel pnpm processes (monorepo installs, CI
 * matrices sharing a cache) can write without coordination.
 */

const CACHE_FILE_NAME = 'minimum-release-age-verified.jsonl'

// Cap the file before it grows large enough to slow down reads. When the
// cap is exceeded we rewrite the file keeping the N most recently verified
// entries. The number is generous — a developer machine that touches a
// thousand distinct lockfiles is far past steady state.
const MAX_CACHE_ENTRIES = 1000

interface CacheRecord {
  lockfilePath: string
  /** sha256 hex of the lockfile content, normalized to LF. */
  lockfileHash: string
  /**
   * Minimum release age (in minutes) the cached verification was run with.
   * A future install with a stricter (larger) cutoff cannot reuse this
   * record — its set of below-cutoff versions may have grown.
   */
  minimumReleaseAge: number
  /** ISO-8601 timestamp of when the verification ran. */
  verifiedAt: string
  /** Lockfile size in bytes — same machine fast path. */
  lockfileFileSize: number
  /**
   * Lockfile mtime in nanoseconds (stringified — JSON numbers lose ns
   * precision). Cross-machine values are meaningless; on a CI runner the
   * fresh checkout resets mtime, so we fall back to hashing. The hash is
   * the source of truth — these stat fields are the local-dev fast path.
   */
  lockfileMtimeNs: string
  lockfileInode: number
}

interface CacheLookupResult {
  hit: boolean
  /** Carried forward when we hit via hash so the caller can refresh stat fields. */
  record?: CacheRecord
}

interface LockfileStat {
  size: number
  mtimeNs: string
  inode: number
}

interface VerificationKey {
  lockfilePath: string
  minimumReleaseAge: number
}

/**
 * Read the most recent record per lockfile path. JSONL is parsed line by
 * line so a malformed line (partial write, disk corruption) doesn't
 * invalidate the rest of the file.
 */
async function readCache (cacheDir: string): Promise<Map<string, CacheRecord>> {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  let contents: string
  try {
    contents = await fs.promises.readFile(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return new Map()
    throw err
  }
  const records = new Map<string, CacheRecord>()
  for (const line of contents.split('\n')) {
    if (!line) continue
    try {
      const parsed = JSON.parse(line) as CacheRecord
      if (typeof parsed?.lockfilePath !== 'string') continue
      // Later records overwrite earlier ones — JSONL semantics.
      records.set(parsed.lockfilePath, parsed)
    } catch {
      // Skip malformed lines; the next clean append will still work.
    }
  }
  return records
}

async function statLockfile (lockfilePath: string): Promise<LockfileStat | null> {
  try {
    const stat = await fs.promises.stat(lockfilePath, { bigint: true })
    return {
      size: Number(stat.size),
      mtimeNs: stat.mtimeNs.toString(),
      inode: Number(stat.ino),
    }
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return null
    throw err
  }
}

/**
 * Try to confirm a cached verification covers the lockfile as it currently
 * sits on disk. Returns `{ hit: true }` to skip the gate; `{ hit: false }`
 * means the caller should run the verifier and persist the result with
 * {@link recordVerification}.
 *
 * The fast path is stat-only (size + mtime + inode) — zero file reads on
 * unchanged repos. The slow path hashes the lockfile only when stat alone
 * can't decide (typically a CI checkout where mtime/inode got reset).
 */
export async function tryLockfileVerificationCache (
  cacheDir: string,
  key: VerificationKey
): Promise<CacheLookupResult> {
  let cache: Map<string, CacheRecord>
  try {
    cache = await readCache(cacheDir)
  } catch (err: unknown) {
    // A corrupt cache file should never block the install; fall through to
    // verification so the gate still runs.
    logger.debug({ msg: 'minimumReleaseAge cache read failed', err })
    return { hit: false }
  }
  const record = cache.get(key.lockfilePath)
  if (!record) return { hit: false }
  // Reusing a record for a weaker cutoff is unsafe: the previously verified
  // set may include versions that no longer meet the stricter policy.
  if (key.minimumReleaseAge > record.minimumReleaseAge) return { hit: false }

  const stat = await statLockfile(key.lockfilePath)
  if (!stat) return { hit: false }
  // Size mismatch is guaranteed-different content; skip the hash entirely.
  if (stat.size !== record.lockfileFileSize) return { hit: false }
  if (stat.mtimeNs === record.lockfileMtimeNs && stat.inode === record.lockfileInode) {
    return { hit: true, record }
  }
  // Stat fields drift (CI checkout, file copy, editor write-through);
  // confirm via content hash, which is portable across machines.
  let hash: string
  try {
    hash = await createHexHashFromFile(key.lockfilePath)
  } catch (err: unknown) {
    logger.debug({ msg: 'minimumReleaseAge cache: lockfile hash failed', err })
    return { hit: false }
  }
  if (hash !== record.lockfileHash) return { hit: false }
  // Hash matched — refresh stat fields so the next install on this machine
  // hits the stat-only fast path.
  await appendRecord(cacheDir, {
    ...record,
    lockfileFileSize: stat.size,
    lockfileMtimeNs: stat.mtimeNs,
    lockfileInode: stat.inode,
  })
  return { hit: true, record }
}

/**
 * Persist a successful verification. Called after the gate passes; the
 * lockfile is hashed once and the resulting record is appended to the
 * cache file. If the file is past {@link MAX_CACHE_ENTRIES}, it is
 * rewritten keeping the most recent entries.
 */
export async function recordVerification (
  cacheDir: string,
  key: VerificationKey
): Promise<void> {
  let stat: LockfileStat | null
  let hash: string
  try {
    stat = await statLockfile(key.lockfilePath)
    if (!stat) return
    hash = await createHexHashFromFile(key.lockfilePath)
  } catch (err: unknown) {
    // The gate has already passed; if we can't record the cache entry we
    // just won't get the speedup next time. Not a reason to fail install.
    logger.debug({ msg: 'minimumReleaseAge cache: could not record verification', err })
    return
  }
  const record: CacheRecord = {
    lockfilePath: key.lockfilePath,
    lockfileHash: hash,
    minimumReleaseAge: key.minimumReleaseAge,
    verifiedAt: new Date().toISOString(),
    lockfileFileSize: stat.size,
    lockfileMtimeNs: stat.mtimeNs,
    lockfileInode: stat.inode,
  }
  await appendRecord(cacheDir, record)
}

async function appendRecord (cacheDir: string, record: CacheRecord): Promise<void> {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  const line = `${JSON.stringify(record)}\n`
  try {
    await fs.promises.mkdir(cacheDir, { recursive: true })
    await fs.promises.appendFile(cacheFilePath, line)
  } catch (err: unknown) {
    logger.debug({ msg: 'minimumReleaseAge cache: append failed', err })
    return
  }
  // Cheap line-count check using the in-memory size estimate: the file may
  // have grown past the cap. Only rewrite when we exceed by a comfortable
  // margin so the rewrite doesn't run on every append.
  await maybeCompactCache(cacheDir)
}

async function maybeCompactCache (cacheDir: string): Promise<void> {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  let contents: string
  try {
    contents = await fs.promises.readFile(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return
    logger.debug({ msg: 'minimumReleaseAge cache: read for compaction failed', err })
    return
  }
  const lines = contents.split('\n').filter(Boolean)
  // 1.5x cap gives us slack so we don't rewrite the file on every append
  // once we cross the cap; the file stays bounded but writes stay cheap.
  if (lines.length <= MAX_CACHE_ENTRIES + (MAX_CACHE_ENTRIES >> 1)) return

  // Last record per path wins (JSONL semantics). Dedupe in reverse so we
  // keep insertion order of the surviving entries; then keep the tail.
  const seen = new Set<string>()
  const reversed: string[] = []
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i]
    try {
      const parsed = JSON.parse(line) as CacheRecord
      if (typeof parsed?.lockfilePath !== 'string') continue
      if (seen.has(parsed.lockfilePath)) continue
      seen.add(parsed.lockfilePath)
      reversed.push(line)
    } catch {
      // Skip malformed lines.
    }
  }
  reversed.reverse()
  const kept = reversed.slice(-MAX_CACHE_ENTRIES)
  try {
    // Write to a sibling tempfile + rename so a concurrent pnpm process
    // can't observe a half-written file.
    const tmpPath = `${cacheFilePath}.${process.pid}.tmp`
    await fs.promises.writeFile(tmpPath, kept.map((line) => `${line}\n`).join(''))
    await fs.promises.rename(tmpPath, cacheFilePath)
  } catch (err: unknown) {
    logger.debug({ msg: 'minimumReleaseAge cache: compaction failed', err })
  }
}

function isNodeError (err: unknown): err is NodeJS.ErrnoException {
  // `instanceof Error` is unreliable across realms (Jest's VM context), so
  // route through util.types.isNativeError per the repo guideline.
  return util.types.isNativeError(err) && 'code' in err
}
