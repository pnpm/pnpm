import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { createHexHash } from '@pnpm/crypto.hash'
import { logger } from '@pnpm/logger'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

/**
 * Subset of {@link ResolutionVerifier} the cache layer needs: the
 * verifier's `policy` contribution plus the `canTrustPastCheck`
 * comparator. `verify` is intentionally absent — the cache never runs
 * verifiers, it just decides whether a previous run is still
 * trustworthy.
 */
export type VerifierCacheIdentity = Pick<ResolutionVerifier, 'policy' | 'canTrustPastCheck'>

/**
 * On-disk cache of verifyLockfileResolutions results, keyed by absolute
 * lockfile path. Lets repeat installs against an unchanged lockfile skip
 * the per-package registry round trips entirely.
 *
 * All filesystem operations are synchronous: the cache is consulted once
 * before verification fan-out and recorded once after — there's no
 * concurrent install work to overlap with, so blocking the event loop
 * for the brief read/stat/hash is fine and keeps the call sites
 * straight-line.
 *
 * Persisted as JSON Lines: each verification appends one record, the
 * latest record per path wins on read. Appends of a single line are
 * atomic on POSIX and NTFS, so parallel pnpm processes (monorepo
 * installs, CI matrices sharing a cache) can write without
 * coordination.
 *
 * Policy-neutral. Every active verifier's `policy` contribution merges
 * into a single `policy` bag on the record; verifiers sharing a logical
 * policy (same field name) share the slot — no resolver-level
 * namespacing.
 */

const CACHE_FILE_NAME = 'lockfile-verified.jsonl'

// Cap the file before it grows large enough to slow down reads. When the
// cap is exceeded we rewrite the file keeping the N most recently verified
// entries. The number is generous — a developer machine that touches a
// thousand distinct lockfiles is far past steady state.
const MAX_CACHE_ENTRIES = 1000

// Records cluster around 250–400 bytes; budget 1 KiB per entry as a
// conservative upper bound. The compaction check uses `stat().size` to
// decide whether to read+rewrite, so we never parse the file unless it
// has actually grown past the cap.
const COMPACT_TRIGGER_BYTES = MAX_CACHE_ENTRIES * 1024 * 3 / 2

interface CacheRecord {
  lockfile: {
    /** Absolute path — the cache key. */
    path: string
    /** sha256 hex of the lockfile content, normalized to LF. */
    hash: string
    /** Lockfile size in bytes — same-machine fast path. */
    size: number
    /**
     * Lockfile mtime in nanoseconds (stringified — JSON numbers lose ns
     * precision). Cross-machine values are meaningless; on a CI runner
     * the fresh checkout resets mtime, so we fall back to hashing. The
     * hash is the source of truth — these stat fields are the
     * local-dev fast path.
     */
    mtimeNs: string
    /**
     * Stringified — some filesystems (e.g. large network drives) use
     * inodes that exceed Number.MAX_SAFE_INTEGER, so a plain number
     * would lose precision and silently invalidate the fast path.
     */
    inode: string
  }
  /** ISO-8601 timestamp of when the verification ran. */
  verifiedAt: string
  /**
   * Merged policy snapshot that passed when the verification ran. Each
   * active {@link VerifierCacheIdentity} contributes its fields here;
   * verifiers checking the same logical policy (same field name) share
   * the slot. On read, each verifier's `canTrustPastCheck` decides
   * whether today's policy can still trust this snapshot.
   */
  policy: Record<string, unknown>
}

interface CacheLookupResult {
  hit: boolean
}

interface LockfileStat {
  size: number
  mtimeNs: string
  inode: string
}

export interface LockfileVerificationCacheKey {
  lockfilePath: string
  verifiers: readonly VerifierCacheIdentity[]
}

/**
 * Read the most recent record per lockfile path. JSONL is parsed line by
 * line so a malformed line (partial write, disk corruption) doesn't
 * invalidate the rest of the file.
 */
function readCache (cacheDir: string): Map<string, CacheRecord> {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  let contents: string
  try {
    contents = fs.readFileSync(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return new Map()
    throw err
  }
  const records = new Map<string, CacheRecord>()
  for (const line of contents.split('\n')) {
    if (!line) continue
    try {
      const parsed = JSON.parse(line) as Partial<CacheRecord>
      const lockfilePath = parsed?.lockfile?.path
      if (typeof lockfilePath !== 'string') continue
      // Later records overwrite earlier ones — JSONL semantics.
      records.set(lockfilePath, normalizeRecord(parsed))
    } catch {
      // Skip malformed lines; the next clean append will still work.
    }
  }
  return records
}

function normalizeRecord (parsed: Partial<CacheRecord>): CacheRecord {
  const lockfile: Partial<CacheRecord['lockfile']> = parsed.lockfile ?? {}
  return {
    lockfile: {
      path: lockfile.path ?? '',
      hash: lockfile.hash ?? '',
      size: lockfile.size ?? -1,
      mtimeNs: lockfile.mtimeNs ?? '',
      inode: lockfile.inode ?? '',
    },
    verifiedAt: parsed.verifiedAt ?? '',
    policy: parsed.policy && typeof parsed.policy === 'object' ? parsed.policy : {},
  }
}

function statLockfile (lockfilePath: string): LockfileStat | null {
  try {
    const stat = fs.statSync(lockfilePath, { bigint: true })
    return {
      size: Number(stat.size),
      mtimeNs: stat.mtimeNs.toString(),
      inode: stat.ino.toString(),
    }
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return null
    throw err
  }
}

function hashLockfile (lockfilePath: string): string {
  // Match createHexHashFromFile: read as UTF-8, normalize CRLF → LF, then
  // hash. Sync because the cache is consulted on a non-parallel path.
  const content = fs.readFileSync(lockfilePath, 'utf8').split('\r\n').join('\n')
  return createHexHash(content)
}

/**
 * Try to confirm a cached verification covers the lockfile as it currently
 * sits on disk and the policies currently in effect. Returns `{ hit: true }`
 * to skip the gate; `{ hit: false }` means the caller should run the
 * verifier and persist the result with {@link recordVerification}.
 *
 * The fast path is stat-only (size + mtime + inode) — zero file reads on
 * unchanged repos. The slow path hashes the lockfile only when stat alone
 * can't decide (typically a CI checkout where mtime/inode got reset).
 *
 * Every active verifier must agree the cached policy snapshot is still
 * trustworthy under what it currently demands; if any rejects, the full
 * gate runs.
 */
export function tryLockfileVerificationCache (
  cacheDir: string,
  key: LockfileVerificationCacheKey
): CacheLookupResult {
  let cache: Map<string, CacheRecord>
  try {
    cache = readCache(cacheDir)
  } catch (err: unknown) {
    // A corrupt cache file should never block the install; fall through to
    // verification so the gate still runs.
    logger.debug({ msg: 'lockfile-verified cache: read failed', err })
    return { hit: false }
  }
  const record = cache.get(key.lockfilePath)
  if (!record) return { hit: false }
  if (!everyVerifierTrustsCachedRun(record, key.verifiers)) return { hit: false }

  const stat = statLockfile(key.lockfilePath)
  if (!stat) return { hit: false }
  // Size mismatch is guaranteed-different content; skip the hash entirely.
  if (stat.size !== record.lockfile.size) return { hit: false }
  if (stat.mtimeNs === record.lockfile.mtimeNs && stat.inode === record.lockfile.inode) {
    return { hit: true }
  }
  // Stat fields drift (CI checkout, file copy, editor write-through);
  // confirm via content hash, which is portable across machines.
  let hash: string
  try {
    hash = hashLockfile(key.lockfilePath)
  } catch (err: unknown) {
    logger.debug({ msg: 'lockfile-verified cache: lockfile hash failed', err })
    return { hit: false }
  }
  if (hash !== record.lockfile.hash) return { hit: false }
  // Hash matched — refresh stat fields so the next install on this machine
  // hits the stat-only fast path.
  appendRecord(cacheDir, {
    ...record,
    lockfile: { ...record.lockfile, size: stat.size, mtimeNs: stat.mtimeNs, inode: stat.inode },
  })
  return { hit: true }
}

function everyVerifierTrustsCachedRun (record: CacheRecord, verifiers: readonly VerifierCacheIdentity[]): boolean {
  for (const verifier of verifiers) {
    if (!verifier.canTrustPastCheck(record.policy)) return false
  }
  return true
}

function mergePolicies (verifiers: readonly VerifierCacheIdentity[]): Record<string, unknown> {
  // Later verifiers overwrite earlier ones on conflict — a shared field
  // should carry the same value across verifiers by convention; mismatch
  // is a config bug and we don't try to reconcile it here.
  const merged: Record<string, unknown> = {}
  for (const verifier of verifiers) {
    Object.assign(merged, verifier.policy)
  }
  return merged
}

/**
 * Persist a successful verification. Called after the gate passes; the
 * lockfile is hashed once and the resulting record is appended to the
 * cache file. If the file is past {@link MAX_CACHE_ENTRIES}, it is
 * rewritten keeping the most recent entries.
 */
export function recordVerification (
  cacheDir: string,
  key: LockfileVerificationCacheKey
): void {
  let stat: LockfileStat | null
  let hash: string
  try {
    stat = statLockfile(key.lockfilePath)
    if (!stat) return
    hash = hashLockfile(key.lockfilePath)
  } catch (err: unknown) {
    // The gate has already passed; if we can't record the cache entry we
    // just won't get the speedup next time. Not a reason to fail install.
    logger.debug({ msg: 'lockfile-verified cache: could not record verification', err })
    return
  }
  const record: CacheRecord = {
    lockfile: {
      path: key.lockfilePath,
      hash,
      size: stat.size,
      mtimeNs: stat.mtimeNs,
      inode: stat.inode,
    },
    verifiedAt: new Date().toISOString(),
    policy: mergePolicies(key.verifiers),
  }
  appendRecord(cacheDir, record)
}

function appendRecord (cacheDir: string, record: CacheRecord): void {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  const line = `${JSON.stringify(record)}\n`
  try {
    fs.mkdirSync(cacheDir, { recursive: true })
    fs.appendFileSync(cacheFilePath, line)
  } catch (err: unknown) {
    logger.debug({ msg: 'lockfile-verified cache: append failed', err })
    return
  }
  maybeCompactCache(cacheDir)
}

function maybeCompactCache (cacheDir: string): void {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  // Decide whether to compact from the file size alone — avoids reading
  // and parsing the file on every successful install. Records cluster
  // around a few hundred bytes; the byte budget translates directly to
  // the entry cap with generous slack so we don't trigger a rewrite on
  // every append once we cross the line.
  let size: number
  try {
    size = fs.statSync(cacheFilePath).size
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return
    logger.debug({ msg: 'lockfile-verified cache: stat for compaction failed', err })
    return
  }
  if (size <= COMPACT_TRIGGER_BYTES) return

  let contents: string
  try {
    contents = fs.readFileSync(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return
    logger.debug({ msg: 'lockfile-verified cache: read for compaction failed', err })
    return
  }
  const lines = contents.split('\n').filter(Boolean)

  // Last record per path wins (JSONL semantics). Dedupe in reverse so we
  // keep insertion order of the surviving entries; then keep the tail.
  const seen = new Set<string>()
  const reversed: string[] = []
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i]
    try {
      const parsed = JSON.parse(line) as Partial<CacheRecord>
      const lockfilePath = parsed?.lockfile?.path
      if (typeof lockfilePath !== 'string') continue
      if (seen.has(lockfilePath)) continue
      seen.add(lockfilePath)
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
    fs.writeFileSync(tmpPath, kept.map((line) => `${line}\n`).join(''))
    fs.renameSync(tmpPath, cacheFilePath)
  } catch (err: unknown) {
    logger.debug({ msg: 'lockfile-verified cache: compaction failed', err })
  }
}

function isNodeError (err: unknown): err is NodeJS.ErrnoException {
  // `instanceof Error` is unreliable across realms (Jest's VM context), so
  // route through util.types.isNativeError per the repo guideline.
  return util.types.isNativeError(err) && 'code' in err
}
