import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { pathToFileURL } from 'node:url'

import { isError, PnpmError } from '@pnpm/error'
import { grantModeBits, mkdirInheritingMode, readDirMode, unixCreationMode } from '@pnpm/store.file-mode'
import { Packr } from 'msgpackr'

import {
  adaptStoreDatabase,
  closeSqliteQuietly,
  createFallbackDatabase,
  isFallbackDatabase,
  isMissingSqliteMethod,
} from './fallbackDatabase.js'

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

function sqliteRetry<Result> (fn: () => Result): Result {
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

function isSqliteBusy (err: unknown): boolean {
  const errcode = (err as { errcode?: unknown } | null | undefined)?.errcode
  // errcode may be an extended error code (e.g. SQLITE_BUSY_RECOVERY = 261),
  // so mask off the upper bits to get the primary error code.
  return typeof errcode === 'number' && (errcode & 0xFF) === SQLITE_BUSY
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
    mkdirInheritingMode(storeDir)
    if (process.platform !== 'win32') createIndexWithInheritedMode(storeDir)
    this.db = adaptStoreDatabase(this.openConnection(storeDir), storeDir)
    try {
      this.configureDatabase()
    } catch (err: unknown) {
      if (isFallbackDatabase(this.db) || !isMissingSqliteMethod(err)) throw err
      closeSqliteQuietly(this.db)
      this.db = createFallbackDatabase(storeDir)
      this.configureDatabase()
    }
  }

  /** Open the host SQLite connection before missing methods are adapted. */
  protected openConnection (storeDir: string): DatabaseSyncType {
    return new DatabaseSync(`${storeDir}/index.db`)
  }

  private configureDatabase (): void {
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

  update (key: string, updateValue: (value: unknown) => unknown): boolean {
    this.flush()
    return sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      try {
        const row = this.stmtGet.get(key) as { data: Uint8Array } | undefined
        if (row == null) {
          this.db.exec('COMMIT')
          return false
        }
        this.stmtSet.run(key, packr.pack(updateValue(packr.unpack(row.data))))
        this.db.exec('COMMIT')
        return true
      } catch (updateError: unknown) {
        try {
          this.db.exec('ROLLBACK')
        } catch (rollbackError: unknown) {
          throw new AggregateError(
            [updateError, rollbackError],
            `Failed to roll back store index update for ${key}`,
            { cause: rollbackError }
          )
        }
        throw updateError
      }
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
    for (const write of writes) {
      this.pendingWrites.push(write)
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

  private inTransaction (action: () => void): void {
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        action()
        this.db.exec('COMMIT')
        committed = true
      } finally {
        this.rollbackIfUncommitted(committed)
      }
    })
  }

  private rollbackIfUncommitted (committed: boolean): void {
    if (committed) return
    try {
      this.db.exec('ROLLBACK')
    } catch {}
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
    this.inTransaction(() => {
      for (const { key, buffer } of entries) {
        this.stmtSet.run(key, buffer)
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
    this.inTransaction(() => {
      for (const key of keys) {
        this.stmtDel.run(key)
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

  /** Run `PRAGMA optimize` before closing. Overridden by {@link ReadOnlyStoreIndex} to skip database writes. */
  protected optimizeBeforeClose (): void {
    try {
      this.db.exec('PRAGMA optimize')
    } catch {
      // PRAGMA optimize is a performance hint; safe to ignore if the DB is locked.
    }
  }
}

/**
 * A read-only connection to a store that other processes may write to.
 * Participates in WAL locking and change detection. SQLite may create sidecar
 * files in the store directory. Use {@link ImmutableStoreIndex} for a frozen
 * store on a read-only filesystem.
 */
export class ReadOnlyStoreIndex extends StoreIndex {
  protected override openDatabase (storeDir: string): void {
    this.db = this.openConnection(storeDir)
    this.db.prepare('PRAGMA busy_timeout=5000').run()
  }

  protected override openConnection (storeDir: string): DatabaseSyncType {
    return new DatabaseSync(`${storeDir}/index.db`, { readOnly: true })
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

  override update (_key: string, _updateValue: (value: unknown) => unknown): boolean {
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

  protected throwReadOnly (): never {
    throw new PnpmError('STORE_READ_ONLY', 'Cannot write to the package store because its index is opened read-only.')
  }
}

/**
 * A frozen store whose database cannot change, including through other
 * processes. Skips SQLite locking and change detection and creates no WAL
 * sidecars, allowing reads on a read-only filesystem. Concurrent writes can
 * cause incorrect results or SQLITE_CORRUPT errors.
 */
export class ImmutableStoreIndex extends ReadOnlyStoreIndex {
  protected override openDatabase (storeDir: string): void {
    if (!nodeSupportsImmutableSqliteUri()) {
      throw new PnpmError(
        'FROZEN_STORE_UNSUPPORTED_NODE',
        `frozenStore opens the store index read-only via a SQLite "immutable" URI, which requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0, but the current version is ${process.versions.node}. Upgrade Node.js, or run without frozenStore.`
      )
    }
    this.db = new DatabaseSync(immutableSqliteUri(`${storeDir}/index.db`))
  }

  protected override throwReadOnly (): never {
    throw new PnpmError('FROZEN_STORE_WRITE', FROZEN_STORE_WRITE_MESSAGE)
  }
}

/**
 * Build the `file://...?immutable=1` URI used to open `index.db` read-only (see
 * the frozen-store rationale at the call site). `pathToFileURL` yields a
 * canonical file URL on every platform: it percent-encodes the URI delimiters
 * that could otherwise truncate the path or inject a query/fragment (`?`, `#`,
 * `%`, spaces) and, on Windows, maps the drive letter and backslashes into a
 * valid `file:///C:/...` form. A raw `file:${path}` concatenation would mis-parse
 * those. See https://sqlite.org/uri.html.
 */
function immutableSqliteUri (dbPath: string): string {
  const url = pathToFileURL(dbPath)
  url.searchParams.set('immutable', '1')
  return url.href
}

/**
 * Whether the running Node.js can open a `file:...?immutable=1` SQLite URI.
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

// SQLite copies the database's mode onto the WAL sidecars, so a new
// index.db gets the store directory's inherited mode, as both the open
// ceiling and the post-create grant, before SQLite opens it.
// The exclusive create decides which process made the database. An existing
// database, including one a concurrent process just created, is not chmod'd.
function createIndexWithInheritedMode (storeDir: string): void {
  const dbPath = path.join(storeDir, 'index.db')
  const creation = unixCreationMode(readDirMode(storeDir), undefined)
  let fd: number
  try {
    fd = fs.openSync(dbPath, 'wx', creation.openMode)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'EEXIST') return
    throw new PnpmError('STORE_DIR_STORE_INDEX_CREATE_FILE', `Failed to create index.db at ${dbPath}: ${isError(err) ? err.message : String(err)}`, { cause: err })
  }
  try {
    if (creation.grantMode != null) grantModeBits(fd, creation.grantMode)
  } catch (err: unknown) {
    // A database left without its inherited mode would be taken as complete
    // by the next open, which skips the grant for an existing file.
    fs.closeSync(fd)
    fs.rmSync(dbPath, { force: true })
    throw err
  }
  fs.closeSync(fd)
}
