import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { logger } from '@pnpm/logger'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

/**
 * Subset of {@link ResolutionVerifier} the cache layer needs: the slot
 * identity (`key`, `policy`) plus the `satisfies` comparator. `verify`
 * is intentionally absent — the cache never runs verifiers, it just
 * decides whether a previous run still applies.
 */
export type VerifierCacheIdentity = Pick<ResolutionVerifier, 'key' | 'policy' | 'satisfies'>

/**
 * On-disk cache of verifyLockfileResolutions results, keyed by absolute
 * lockfile path. Lets repeat installs against an unchanged lockfile skip
 * the per-package registry round trips entirely.
 *
 * Persisted as JSON Lines: each verification appends one record, the latest
 * record per path wins on read. Appends of a single line are atomic on
 * POSIX and NTFS, so parallel pnpm processes (monorepo installs, CI
 * matrices sharing a cache) can write without coordination.
 *
 * Policy-neutral. Each {@link VerifierCacheIdentity} contributes its own slot
 * under `verifiers[key]`; future verifiers add their own keys without
 * touching the cache layer.
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
  lockfilePath: string
  /** sha256 hex of the lockfile content, normalized to LF. */
  lockfileHash: string
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
  /**
   * Verifier-keyed policy snapshots that were satisfied when the
   * verification ran. Each {@link VerifierCacheIdentity} owns its own slot and
   * decides — via its `satisfies` comparator — whether today's policy can
   * reuse the cached snapshot. Unknown keys are ignored on read.
   */
  verifiers: Record<string, unknown>
}

interface CacheLookupResult {
  hit: boolean
}

interface LockfileStat {
  size: number
  mtimeNs: string
  inode: number
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
      const parsed = JSON.parse(line) as Partial<CacheRecord>
      if (typeof parsed?.lockfilePath !== 'string') continue
      // Later records overwrite earlier ones — JSONL semantics.
      records.set(parsed.lockfilePath, normalizeRecord(parsed))
    } catch {
      // Skip malformed lines; the next clean append will still work.
    }
  }
  return records
}

function normalizeRecord (parsed: Partial<CacheRecord>): CacheRecord {
  return {
    lockfilePath: parsed.lockfilePath ?? '',
    lockfileHash: parsed.lockfileHash ?? '',
    verifiedAt: parsed.verifiedAt ?? '',
    lockfileFileSize: parsed.lockfileFileSize ?? -1,
    lockfileMtimeNs: parsed.lockfileMtimeNs ?? '',
    lockfileInode: parsed.lockfileInode ?? -1,
    verifiers: parsed.verifiers && typeof parsed.verifiers === 'object' ? parsed.verifiers : {},
  }
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
 * sits on disk and the policies currently in effect. Returns `{ hit: true }`
 * to skip the gate; `{ hit: false }` means the caller should run the
 * verifier and persist the result with {@link recordVerification}.
 *
 * The fast path is stat-only (size + mtime + inode) — zero file reads on
 * unchanged repos. The slow path hashes the lockfile only when stat alone
 * can't decide (typically a CI checkout where mtime/inode got reset).
 *
 * Every active verifier must agree the cached snapshot still satisfies its
 * current policy; if any verifier rejects (or its slot is missing), the
 * full gate runs.
 */
export async function tryLockfileVerificationCache (
  cacheDir: string,
  key: LockfileVerificationCacheKey
): Promise<CacheLookupResult> {
  let cache: Map<string, CacheRecord>
  try {
    cache = await readCache(cacheDir)
  } catch (err: unknown) {
    // A corrupt cache file should never block the install; fall through to
    // verification so the gate still runs.
    logger.debug({ msg: 'lockfile-verified cache: read failed', err })
    return { hit: false }
  }
  const record = cache.get(key.lockfilePath)
  if (!record) return { hit: false }
  if (!everyVerifierSatisfied(record, key.verifiers)) return { hit: false }

  const stat = await statLockfile(key.lockfilePath)
  if (!stat) return { hit: false }
  // Size mismatch is guaranteed-different content; skip the hash entirely.
  if (stat.size !== record.lockfileFileSize) return { hit: false }
  if (stat.mtimeNs === record.lockfileMtimeNs && stat.inode === record.lockfileInode) {
    return { hit: true }
  }
  // Stat fields drift (CI checkout, file copy, editor write-through);
  // confirm via content hash, which is portable across machines.
  let hash: string
  try {
    hash = await createHexHashFromFile(key.lockfilePath)
  } catch (err: unknown) {
    logger.debug({ msg: 'lockfile-verified cache: lockfile hash failed', err })
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
  return { hit: true }
}

function everyVerifierSatisfied (record: CacheRecord, verifiers: readonly VerifierCacheIdentity[]): boolean {
  for (const verifier of verifiers) {
    // Missing slot is treated as "not satisfied" — the cached run didn't
    // cover this verifier so we must rerun the gate.
    if (!(verifier.key in record.verifiers)) return false
    if (!verifier.satisfies(record.verifiers[verifier.key])) return false
  }
  return true
}

/**
 * Persist a successful verification. Called after the gate passes; the
 * lockfile is hashed once and the resulting record is appended to the
 * cache file. If the file is past {@link MAX_CACHE_ENTRIES}, it is
 * rewritten keeping the most recent entries.
 */
export async function recordVerification (
  cacheDir: string,
  key: LockfileVerificationCacheKey
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
    logger.debug({ msg: 'lockfile-verified cache: could not record verification', err })
    return
  }
  const verifierSlots: Record<string, unknown> = {}
  for (const verifier of key.verifiers) {
    verifierSlots[verifier.key] = verifier.policy
  }
  const record: CacheRecord = {
    lockfilePath: key.lockfilePath,
    lockfileHash: hash,
    verifiedAt: new Date().toISOString(),
    lockfileFileSize: stat.size,
    lockfileMtimeNs: stat.mtimeNs,
    lockfileInode: stat.inode,
    verifiers: verifierSlots,
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
    logger.debug({ msg: 'lockfile-verified cache: append failed', err })
    return
  }
  await maybeCompactCache(cacheDir)
}

async function maybeCompactCache (cacheDir: string): Promise<void> {
  const cacheFilePath = path.join(cacheDir, CACHE_FILE_NAME)
  // Decide whether to compact from the file size alone — avoids reading
  // and parsing the file on every successful install. Records cluster
  // around a few hundred bytes; the byte budget translates directly to
  // the entry cap with generous slack so we don't trigger a rewrite on
  // every append once we cross the line.
  let size: number
  try {
    const stat = await fs.promises.stat(cacheFilePath)
    size = stat.size
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return
    logger.debug({ msg: 'lockfile-verified cache: stat for compaction failed', err })
    return
  }
  if (size <= COMPACT_TRIGGER_BYTES) return

  let contents: string
  try {
    contents = await fs.promises.readFile(cacheFilePath, 'utf8')
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
    logger.debug({ msg: 'lockfile-verified cache: compaction failed', err })
  }
}

function isNodeError (err: unknown): err is NodeJS.ErrnoException {
  // `instanceof Error` is unreliable across realms (Jest's VM context), so
  // route through util.types.isNativeError per the repo guideline.
  return util.types.isNativeError(err) && 'code' in err
}
