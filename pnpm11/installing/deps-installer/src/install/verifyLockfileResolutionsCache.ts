import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

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
 * On-disk cache of verifyLockfileResolutions results, keyed by lockfile
 * content hash. Lets repeat installs against an unchanged lockfile skip
 * the per-package registry round trips entirely — including across git
 * worktrees, where the same lockfile content lives at different paths.
 *
 * All filesystem operations are synchronous: the cache is consulted
 * once before verification fan-out and recorded once after — there's
 * no concurrent install work to overlap with, so blocking the event
 * loop for the brief read/stat/hash is fine and keeps the call sites
 * straight-line.
 *
 * Persisted as JSON Lines: each verification appends one record;
 * later records overwrite earlier ones on key collision when read.
 * Appends of a single line are atomic on POSIX and NTFS, so parallel
 * pnpm processes (monorepo installs, CI matrices sharing a cache) can
 * write without coordination.
 *
 * Policy-neutral. Every active verifier's `policy` contribution merges
 * into a single `policy` bag on the record; verifiers sharing a
 * logical policy (same field name) share the slot — no resolver-level
 * namespacing.
 */

const CACHE_FILE_NAME = 'lockfile-verified.jsonl'

// Cap the file before it grows large enough to slow down reads. When the
// cap is exceeded we rewrite the file keeping the N most recently
// verified entries. The number is generous — a developer machine that
// touches a thousand distinct (path, content) tuples is far past steady
// state.
const MAX_CACHE_ENTRIES = 1000

// Records cluster around 250–400 bytes; budget 1 KiB per entry as a
// conservative upper bound.
const COMPACT_TRIGGER_BYTES = MAX_CACHE_ENTRIES * 1024 * 3 / 2

interface CacheRecord {
  lockfile: {
    /**
     * sha256 hex of the lockfile content — primary cache key. Computed
     * from the parsed in-memory lockfile object (not the raw file
     * bytes); two YAML layouts that parse to the same object share a
     * hash. Same content on disk → same parsed object → same hash, so
     * worktrees and CI checkouts collide here.
     */
    hash: string
    /** Absolute path the cache last saw this content at — secondary index for the stat fast path. */
    path: string
    /** Lockfile size in bytes. */
    size: number
    /**
     * Lockfile mtime in nanoseconds (stringified — JSON numbers lose
     * ns precision). Cross-machine values are meaningless; on a CI
     * runner the fresh checkout resets mtime, so we fall back to
     * hashing.
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

export interface CacheLookupResult {
  hit: boolean
  /**
   * ISO-8601 timestamp of the verification run the hit is reusing.
   * Set only on a hit, and only when the record carries a usable
   * timestamp (records written before the field existed normalize to
   * an empty string and surface as `undefined` here).
   */
  verifiedAt?: string
  /**
   * stat + hash already computed during the lookup. When the caller
   * follows up with {@link recordVerification} after running the gate,
   * passing these back avoids re-stat'ing and (especially) re-hashing
   * the lockfile a second time. Fields are undefined when the lookup
   * couldn't (or didn't need to) compute them — `recordVerification`
   * falls back to computing what's missing.
   */
  precomputed: { stat?: LockfileStat, hash?: string }
}

interface LockfileStat {
  size: number
  mtimeNs: string
  inode: string
}

export interface LockfileVerificationCacheKey {
  lockfilePath: string
  verifiers: readonly VerifierCacheIdentity[]
  /**
   * Lazy: returns a stable hex hash of the in-memory lockfile. The
   * cache invokes this only when the stat shortcut doesn't apply (the
   * lockfile is at a new path, or its stat has drifted from the
   * cached record). When the stat shortcut hits, the in-memory hash is
   * never computed.
   */
  hashLockfile: () => string
}

interface CacheIndexes {
  /** Latest record per content hash — primary lookup. */
  byHash: Map<string, CacheRecord>
  /** Latest record per absolute path — same-machine stat fast path. */
  byPath: Map<string, CacheRecord>
}

/**
 * Build two indexes over the JSONL records in one pass: by content
 * hash (primary) and by absolute path (stat shortcut). Records are
 * walked in file order so the last record for any key wins.
 */
function readCache (cacheDir: string): CacheIndexes {
  const byHash = new Map<string, CacheRecord>()
  const byPath = new Map<string, CacheRecord>()
  const contents = readCacheFileIfExists(path.join(cacheDir, CACHE_FILE_NAME))
  if (contents == null) return { byHash, byPath }
  for (const line of contents.split('\n')) {
    if (!line) continue
    const record = parseCacheLine(line)
    if (!record) continue
    byHash.set(record.lockfile.hash, record)
    byPath.set(record.lockfile.path, record)
  }
  return { byHash, byPath }
}

function readCacheFileIfExists (cacheFilePath: string): string | undefined {
  try {
    return fs.readFileSync(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return undefined
    throw err
  }
}

/**
 * Returns `undefined` for a malformed line or one missing the hash/path
 * keys; the next clean append will still work.
 */
function parseCacheLine (line: string): CacheRecord | undefined {
  try {
    const parsed = JSON.parse(line) as Partial<CacheRecord>
    if (!hasRecordKeys(parsed)) return undefined
    return normalizeRecord(parsed)
  } catch {
    return undefined
  }
}

type KeyedRecord = Partial<CacheRecord> & { lockfile: Pick<CacheRecord['lockfile'], 'hash' | 'path'> }

function hasRecordKeys (parsed: Partial<CacheRecord> | null): parsed is KeyedRecord {
  return typeof parsed?.lockfile?.hash === 'string' && typeof parsed?.lockfile?.path === 'string'
}

function normalizeRecord (parsed: Partial<CacheRecord>): CacheRecord {
  const lockfile: Partial<CacheRecord['lockfile']> = parsed.lockfile ?? {}
  return {
    lockfile: {
      hash: lockfile.hash ?? '',
      path: lockfile.path ?? '',
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

function statMatches (stat: LockfileStat, lockfile: CacheRecord['lockfile']): boolean {
  return stat.size === lockfile.size &&
    stat.mtimeNs === lockfile.mtimeNs &&
    stat.inode === lockfile.inode
}

/**
 * Try to confirm a cached verification covers the lockfile as it
 * currently sits on disk and the policies currently in effect. Returns
 * `{ hit: true }` to skip the gate; `{ hit: false }` means the caller
 * should run the verifier and persist the result with
 * {@link recordVerification}.
 *
 * Lookup order:
 *
 * 1. **Stat shortcut** — if we've previously verified this exact path
 *    with these exact stat values, trust the cached hash and skip
 *    reading the lockfile.
 * 2. **Content lookup** — hash the lockfile and look up by hash.
 *    Catches the worktree case (same content, different path) and
 *    CI checkouts where stat fields got reset. Refreshes the
 *    stat-shortcut entry on hit so the next install at this path
 *    skips the hash.
 *
 * Every active verifier must agree the cached policy snapshot is still
 * trustworthy under what it currently demands; if any rejects, the
 * full gate runs.
 */
export function tryLockfileVerificationCache (
  cacheDir: string,
  key: LockfileVerificationCacheKey
): CacheLookupResult {
  let indexes: CacheIndexes
  try {
    indexes = readCache(cacheDir)
  } catch (err: unknown) {
    // A corrupt cache file should never block the install; fall
    // through to verification so the gate still runs.
    logger.debug({ msg: 'lockfile-verified cache: read failed', err })
    return { hit: false, precomputed: {} }
  }

  const stat = statLockfile(key.lockfilePath)
  if (!stat) return { hit: false, precomputed: {} }

  const byPathRecord = indexes.byPath.get(key.lockfilePath)
  if (byPathRecord && statMatches(stat, byPathRecord.lockfile)) {
    return lookupByStat({ record: byPathRecord, stat, verifiers: key.verifiers })
  }
  return lookupByContentHash({ cacheDir, indexes, key, stat })
}

function lookupByStat (
  { record, stat, verifiers }: { record: CacheRecord, stat: LockfileStat, verifiers: readonly VerifierCacheIdentity[] }
): CacheLookupResult {
  const hit = everyVerifierTrustsCachedRun(record, verifiers)
  return {
    hit,
    verifiedAt: hit ? record.verifiedAt || undefined : undefined,
    // The stat-match implies the file content is unchanged since the
    // cached record was written, so its hash is still correct. Pass
    // it through to skip hashing on the miss-then-record path.
    precomputed: { stat, hash: record.lockfile.hash },
  }
}

interface ContentHashLookupOptions {
  cacheDir: string
  indexes: CacheIndexes
  key: LockfileVerificationCacheKey
  stat: LockfileStat
}

function lookupByContentHash ({ cacheDir, indexes, key, stat }: ContentHashLookupOptions): CacheLookupResult {
  let hash: string
  try {
    hash = key.hashLockfile()
  } catch (err: unknown) {
    logger.debug({ msg: 'lockfile-verified cache: lockfile hash failed', err })
    return { hit: false, precomputed: { stat } }
  }
  const byHashRecord = indexes.byHash.get(hash)
  if (!byHashRecord) return { hit: false, precomputed: { stat, hash } }
  if (!everyVerifierTrustsCachedRun(byHashRecord, key.verifiers)) {
    return { hit: false, precomputed: { stat, hash } }
  }

  appendRecord(cacheDir, {
    ...byHashRecord,
    lockfile: { ...byHashRecord.lockfile, path: key.lockfilePath, size: stat.size, mtimeNs: stat.mtimeNs, inode: stat.inode },
  })
  return { hit: true, verifiedAt: byHashRecord.verifiedAt || undefined, precomputed: { stat, hash } }
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
  key: LockfileVerificationCacheKey,
  precomputed?: { stat?: LockfileStat, hash?: string }
): void {
  let stat: LockfileStat | null
  let hash: string
  try {
    stat = precomputed?.stat ?? statLockfile(key.lockfilePath)
    if (!stat) return
    hash = precomputed?.hash ?? key.hashLockfile()
  } catch (err: unknown) {
    // The gate has already passed; if we can't record the cache entry we
    // just won't get the speedup next time. Not a reason to fail install.
    logger.debug({ msg: 'lockfile-verified cache: could not record verification', err })
    return
  }
  const record: CacheRecord = {
    lockfile: {
      hash,
      path: key.lockfilePath,
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
  if (!cacheExceedsCompactionTrigger(cacheFilePath)) return
  const contents = readCacheForCompaction(cacheFilePath)
  if (contents == null) return
  const kept = keepNewestRecordPerTuple(contents.split('\n').filter(Boolean)).slice(-MAX_CACHE_ENTRIES)
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

/**
 * Decide whether to compact from the file size alone — avoids reading
 * and parsing the file on every successful install.
 */
function cacheExceedsCompactionTrigger (cacheFilePath: string): boolean {
  let size: number
  try {
    size = fs.statSync(cacheFilePath).size
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return false
    logger.debug({ msg: 'lockfile-verified cache: stat for compaction failed', err })
    return false
  }
  return size > COMPACT_TRIGGER_BYTES
}

function readCacheForCompaction (cacheFilePath: string): string | undefined {
  try {
    return fs.readFileSync(cacheFilePath, 'utf8')
  } catch (err: unknown) {
    if (isNodeError(err) && err.code === 'ENOENT') return undefined
    logger.debug({ msg: 'lockfile-verified cache: read for compaction failed', err })
    return undefined
  }
}

/**
 * Dedup by (path, hash) — that's the unit both indexes care about.
 * Walking reverse keeps the newest record per tuple; the result is in
 * original order. Malformed lines are dropped.
 */
function keepNewestRecordPerTuple (lines: string[]): string[] {
  const seen = new Set<string>()
  const reversed: string[] = []
  for (let index = lines.length - 1; index >= 0; index--) {
    const line = lines[index]
    const tupleKey = readRecordTupleKey(line)
    if (tupleKey == null || seen.has(tupleKey)) continue
    seen.add(tupleKey)
    reversed.push(line)
  }
  return reversed.reverse()
}

function readRecordTupleKey (line: string): string | undefined {
  try {
    const parsed = JSON.parse(line) as Partial<CacheRecord>
    if (!hasRecordKeys(parsed)) return undefined
    return `${parsed.lockfile.path} ${parsed.lockfile.hash}`
  } catch {
    return undefined
  }
}

function isNodeError (err: unknown): err is NodeJS.ErrnoException {
  // `instanceof Error` is unreliable across realms (Jest's VM context), so
  // route through util.types.isNativeError per the repo guideline.
  return util.types.isNativeError(err) && 'code' in err
}
