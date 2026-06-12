import fs from 'node:fs'
import { createRequire } from 'node:module'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { pathToFileURL } from 'node:url'

import { PnpmError } from '@pnpm/error'
import { Packr } from 'msgpackr'

const FROZEN_STORE_WRITE_MESSAGE = 'Cannot write to the package store because frozenStore is enabled (the store is opened read-only). This indicates the store is missing content the install needs.'

// Use createRequire to load node:sqlite because it is a prefix-only builtin
// that Jest's ESM module resolver cannot handle.
const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

const packr = new Packr({
  useRecords: true,
  moreTypes: true,
})

const SQLITE_BUSY = 5
const RETRY_DELAY_MS = 50
const MAX_RETRIES = 100 // ~5 seconds total

function sqliteRetry<T> (fn: () => T): T {
  for (let attempt = 0; ; attempt++) {
    try {
      return fn()
    } catch (err: unknown) {
      if (isSqliteBusy(err) && attempt < MAX_RETRIES) {
        sleepSync(RETRY_DELAY_MS)
        continue
      }
      throw err
    }
  }
}

function isSqliteBusy (err: any): boolean { // eslint-disable-line @typescript-eslint/no-explicit-any
  // errcode may be an extended error code (e.g. SQLITE_BUSY_RECOVERY = 261),
  // so mask off the upper bits to get the primary error code.
  return (err?.errcode & 0xFF) === SQLITE_BUSY
}

const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))

function sleepSync (ms: number): void {
  Atomics.wait(sleepBuffer, 0, 0, ms)
}

/**
 * Pack data for storage using msgpackr.
 * Use this when data will be packed in one thread and stored by another,
 * to ensure the same Packr instance is used for pack and unpack within each thread.
 */
export function packForStorage (data: unknown): Uint8Array {
  return packr.pack(data)
}

/**
 * Create a store index key from an integrity hash and package id.
 * The key is `${integrity}\t${pkgId}` — tab-separated.
 * Integrity strings never contain tabs, so this is unambiguous.
 */
export function storeIndexKey (integrity: string, pkgId: string): string {
  return `${integrity}\t${pkgId}`
}

export function gitHostedStoreIndexKey (pkgId: string, opts: { built: boolean }): string {
  return storeIndexKey(pkgId, opts.built ? 'built' : 'not-built')
}

/**
 * Pick the store index key for a tarball-shaped resolution.
 *
 * Git-hosted tarballs (`resolution.gitHosted === true`) are addressed by
 * `gitHostedStoreIndexKey(pkgId, { built })` — their cached content depends
 * on whether build scripts ran during fetch (`preparePackage`), so the
 * `built` dimension is part of the key. The integrity-only key would
 * collapse the built/not-built variants into one slot.
 *
 * Tarballs with integrity that aren't git-hosted are addressed by
 * `storeIndexKey(integrity, pkgId)`.
 *
 * Resolutions that have neither flag fall through to
 * `gitHostedStoreIndexKey` — these are typically lockfile entries written
 * by older pnpm versions that lacked integrity.
 */
export function pickStoreIndexKey (
  resolution: { gitHosted?: boolean, integrity?: string },
  pkgId: string,
  opts: { built: boolean }
): string {
  if (resolution.gitHosted || !resolution.integrity) {
    return gitHostedStoreIndexKey(pkgId, opts)
  }
  return storeIndexKey(resolution.integrity, pkgId)
}

const openInstances = new Set<StoreIndex>()

/**
 * Close all open StoreIndex instances.
 * Useful in tests that need to remove the store directory.
 */
export function closeAllStoreIndexes (): void {
  for (const si of openInstances) {
    si.close()
  }
}

export class StoreIndex {
  protected db!: DatabaseSyncType
  protected closed = false
  private pendingWrites: Array<{ key: string, buffer: Uint8Array }> = []
  private flushScheduled = false
  protected stmtGet!: StatementSync
  protected stmtSet!: StatementSync
  protected stmtDel!: StatementSync
  protected stmtHas!: StatementSync
  protected stmtAll!: StatementSync
  protected stmtKeys!: StatementSync
  private readonly exitHandler: () => void

  constructor (storeDir: string) {
    this.openDatabase(storeDir)
    this.prepareStatements()
    this.exitHandler = () => this.close()
    // Multiple StoreIndex instances may be created (e.g. in tests), each adding
    // an exit listener. Raise the limit to avoid MaxListenersExceededWarning.
    // Skip when maxListeners is 0 (unlimited).
    const currentMax = process.getMaxListeners()
    if (currentMax !== 0 && currentMax < openInstances.size + 11) {
      process.setMaxListeners(Math.max(currentMax + 10, openInstances.size + 11))
    }
    process.on('exit', this.exitHandler)
    openInstances.add(this)
  }

  /** Open the SQLite connection. Overridden by {@link ReadOnlyStoreIndex}. */
  protected openDatabase (storeDir: string): void {
    fs.mkdirSync(storeDir, { recursive: true })
    this.db = new DatabaseSync(`${storeDir}/index.db`)
    // Set busy_timeout FIRST so SQLite's internal busy handler is active
    // during all subsequent operations. On Windows, file locking is mandatory
    // and concurrent processes (e.g. parallel dlx calls) will contend.
    this.db.exec('PRAGMA busy_timeout=5000')
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
      this.db.exec('PRAGMA synchronous=NORMAL')
      // Increase memory map size to 512MB
      this.db.exec('PRAGMA mmap_size=536870912')
      // Increase page cache size to ~32MB
      this.db.exec('PRAGMA cache_size=-32000')
      this.db.exec('PRAGMA temp_store=MEMORY')
      // Increase wal autocheckpoint interval to reduce I/O during heavy writes
      this.db.exec('PRAGMA wal_autocheckpoint=10000')
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS package_index (
          key TEXT PRIMARY KEY,
          data BLOB NOT NULL
        ) WITHOUT ROWID
      `)
    })
  }

  /** Prepare the prepared statements. Overridden by {@link ReadOnlyStoreIndex} to skip the write statements. */
  protected prepareStatements (): void {
    this.stmtGet = this.db.prepare('SELECT data FROM package_index WHERE key = ?')
    this.stmtSet = this.db.prepare('INSERT OR REPLACE INTO package_index (key, data) VALUES (?, ?)')
    this.stmtDel = this.db.prepare('DELETE FROM package_index WHERE key = ?')
    this.stmtHas = this.db.prepare('SELECT 1 FROM package_index WHERE key = ?')
    this.stmtAll = this.db.prepare('SELECT key, data FROM package_index')
    this.stmtKeys = this.db.prepare('SELECT key FROM package_index')
  }

  get (key: string): unknown | undefined {
    const row = sqliteRetry(() => this.stmtGet.get(key)) as { data: Uint8Array } | undefined
    if (row) {
      return packr.unpack(row.data)
    }
    return undefined
  }

  /**
   * Get the raw msgpack-encoded buffer for a key without decoding.
   */
  getRaw (key: string): Uint8Array | undefined {
    const row = sqliteRetry(() => this.stmtGet.get(key)) as { data: Uint8Array } | undefined
    return row?.data
  }

  set (key: string, data: unknown): void {
    const buffer = packr.pack(data)
    sqliteRetry(() => {
      this.stmtSet.run(key, buffer)
    })
  }

  delete (key: string): boolean {
    let result!: { changes: number | bigint }
    sqliteRetry(() => {
      result = this.stmtDel.run(key)
    })
    return result.changes > 0
  }

  has (key: string): boolean {
    return sqliteRetry(() => this.stmtHas.get(key)) != null
  }

  /**
   * Iterate over all index entries.
   * Yields [key, data] pairs where key is `integrity\tpkgId`.
   */
  * entries (): IterableIterator<[string, unknown]> {
    for (const row of this.stmtAll.iterate() as IterableIterator<{ key: string, data: Uint8Array }>) {
      yield [row.key, packr.unpack(row.data)]
    }
  }

  /**
   * Iterate over all index keys without decoding values.
   * Much faster than entries() when only keys are needed.
   */
  * keys (): IterableIterator<string> {
    for (const row of this.stmtKeys.iterate() as IterableIterator<{ key: string }>) {
      yield row.key
    }
  }

  /**
   * Queue pre-packed writes to be flushed on the next tick.
   * Used by the fetch phase for throughput.
   */
  queueWrites (writes: Array<{ key: string, buffer: Uint8Array }>): void {
    for (const w of writes) {
      this.pendingWrites.push(w)
    }
    if (!this.flushScheduled) {
      this.flushScheduled = true
      process.nextTick(() => this.flush())
    }
  }

  /**
   * Flush all pending queued writes immediately.
   */
  flush (): void {
    this.flushScheduled = false
    if (this.pendingWrites.length === 0) return
    this.setRawMany(this.pendingWrites)
    this.pendingWrites = []
  }

  /**
   * Write multiple pre-packed entries in a single transaction.
   * The buffers must already be msgpack-encoded.
   */
  setRawMany (entries: Array<{ key: string, buffer: Uint8Array }>): void {
    if (this.closed || entries.length === 0) return
    if (entries.length === 1) {
      sqliteRetry(() => {
        this.stmtSet.run(entries[0].key, entries[0].buffer)
      })
      return
    }
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const { key, buffer } of entries) {
          this.stmtSet.run(key, buffer)
        }
        this.db.exec('COMMIT')
        committed = true
      } finally {
        if (!committed) {
          try {
            this.db.exec('ROLLBACK')
          } catch {}
        }
      }
    })
  }

  /**
   * Delete multiple index entries in a single transaction,
   * then VACUUM to reclaim disk space.
   */
  deleteMany (keys: string[]): void {
    if (keys.length === 0) return
    if (keys.length === 1) {
      this.delete(keys[0])
      this.db.exec('VACUUM')
      return
    }
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const key of keys) {
          this.stmtDel.run(key)
        }
        this.db.exec('COMMIT')
        committed = true
      } finally {
        if (!committed) {
          try {
            this.db.exec('ROLLBACK')
          } catch {}
        }
      }
    })
    this.db.exec('VACUUM')
  }

  checkpoint (): void {
    this.flush()
    // wal_checkpoint can hit SQLITE_BUSY if another process is reading the
    // same index.db concurrently. Retry for consistency with other ops here.
    sqliteRetry(() => {
      this.db.exec('PRAGMA wal_checkpoint(TRUNCATE)')
    })
  }

  close (): void {
    if (this.closed) return
    this.flush()
    this.closed = true
    openInstances.delete(this)
    process.removeListener('exit', this.exitHandler)
    this.optimizeBeforeClose()
    try {
      this.db.close()
    } catch {
      // The DB may be locked by another connection; the OS will reclaim it on process exit.
    }
  }

  /** Run `PRAGMA optimize` before closing. Overridden by {@link ReadOnlyStoreIndex} to skip it (the DB is immutable). */
  protected optimizeBeforeClose (): void {
    try {
      this.db.exec('PRAGMA optimize')
    } catch {
      // PRAGMA optimize is a performance hint; safe to ignore if the DB is locked.
    }
  }
}

/**
 * A {@link StoreIndex} opened read-only for installs against a store on a
 * read-only filesystem (`frozenStore`). The index is a WAL-mode database, and a
 * normal WAL read creates an `index.db-shm` sidecar in the store directory —
 * which fails on a read-only directory and surfaces as "attempt to write a
 * readonly database" on the first query. Opening via the SQLite `immutable=1`
 * URI tells SQLite the file cannot change, so it bypasses the WAL/shm machinery
 * and reads the file directly, creating no sidecars.
 *
 * The store is assumed complete; every write is a programming error and throws.
 */
export class ReadOnlyStoreIndex extends StoreIndex {
  protected override openDatabase (storeDir: string): void {
    if (!nodeSupportsImmutableSqliteUri()) {
      throw new PnpmError(
        'FROZEN_STORE_UNSUPPORTED_NODE',
        `frozenStore opens the store index read-only via a SQLite "immutable" URI, which requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0, but the current version is ${process.versions.node}. Upgrade Node.js, or run without frozenStore.`
      )
    }
    this.db = new DatabaseSync(immutableSqliteUri(`${storeDir}/index.db`))
  }

  protected override prepareStatements (): void {
    this.stmtGet = this.db.prepare('SELECT data FROM package_index WHERE key = ?')
    this.stmtHas = this.db.prepare('SELECT 1 FROM package_index WHERE key = ?')
    this.stmtAll = this.db.prepare('SELECT key, data FROM package_index')
    this.stmtKeys = this.db.prepare('SELECT key FROM package_index')
  }

  protected override optimizeBeforeClose (): void {}

  override set (_key: string, _data: unknown): void {
    this.throwReadOnly()
  }

  override delete (_key: string): boolean {
    this.throwReadOnly()
  }

  override queueWrites (_writes: Array<{ key: string, buffer: Uint8Array }>): void {
    this.throwReadOnly()
  }

  override setRawMany (_entries: Array<{ key: string, buffer: Uint8Array }>): void {
    this.throwReadOnly()
  }

  override deleteMany (_keys: string[]): void {
    this.throwReadOnly()
  }

  override checkpoint (): void {
    this.throwReadOnly()
  }

  private throwReadOnly (): never {
    throw new PnpmError('FROZEN_STORE_WRITE', FROZEN_STORE_WRITE_MESSAGE)
  }
}

/**
 * Build the `file://…?immutable=1` URI used to open `index.db` read-only (see
 * the frozen-store rationale at the call site). `pathToFileURL` yields a
 * canonical file URL on every platform: it percent-encodes the URI delimiters
 * that could otherwise truncate the path or inject a query/fragment (`?`, `#`,
 * `%`, spaces) and, on Windows, maps the drive letter and backslashes into a
 * valid `file:///C:/…` form. A raw `file:${path}` concatenation would mis-parse
 * those. See https://sqlite.org/uri.html.
 */
function immutableSqliteUri (dbPath: string): string {
  const url = pathToFileURL(dbPath)
  url.searchParams.set('immutable', '1')
  return url.href
}

/**
 * Whether the running Node.js can open a `file:…?immutable=1` SQLite URI.
 *
 * `node:sqlite` only passes `SQLITE_OPEN_URI` to SQLite — so the `immutable=1`
 * query is honored rather than treated as part of a literal filename — starting
 * in v22.15.0 (22.x line), v23.11.0 (23.x line), and every v24+. On older
 * runtimes the URI is opened as a literal path and fails with a cryptic
 * "unable to open database file"; we detect that up front to give actionable
 * guidance instead.
 */
function nodeSupportsImmutableSqliteUri (): boolean {
  const [major, minor] = process.versions.node.split('.', 2).map(Number)
  if (major < 22) return false
  if (major === 22) return minor >= 15
  if (major === 23) return minor >= 11
  return true
}
