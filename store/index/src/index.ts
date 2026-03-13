import fs from 'node:fs'
import { createRequire } from 'node:module'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'

import { Packr } from 'msgpackr'

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
  private db: DatabaseSyncType
  private closed = false
  private pendingWrites: Array<{ key: string, buffer: Uint8Array }> = []
  private flushScheduled = false
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtDel: StatementSync
  private stmtHas: StatementSync
  private stmtAll: StatementSync
  private readonly exitHandler: () => void

  constructor (storeDir: string) {
    const dbPath = `${storeDir}/index.db`
    fs.mkdirSync(storeDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
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
    this.stmtGet = this.db.prepare('SELECT data FROM package_index WHERE key = ?')
    this.stmtSet = this.db.prepare('INSERT OR REPLACE INTO package_index (key, data) VALUES (?, ?)')
    this.stmtDel = this.db.prepare('DELETE FROM package_index WHERE key = ?')
    this.stmtHas = this.db.prepare('SELECT 1 FROM package_index WHERE key = ?')
    this.stmtAll = this.db.prepare('SELECT key, data FROM package_index')
    this.exitHandler = () => this.close()
    process.on('exit', this.exitHandler)
    openInstances.add(this)
  }

  get (key: string): unknown | undefined {
    const row = sqliteRetry(() => this.stmtGet.get(key)) as { data: Uint8Array } | undefined
    if (row) {
      return packr.unpack(row.data)
    }
    return undefined
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

  close (): void {
    if (this.closed) return
    this.flush()
    this.closed = true
    openInstances.delete(this)
    process.removeListener('exit', this.exitHandler)
    try {
      this.db.exec('PRAGMA optimize')
    } catch {
      // PRAGMA optimize is a performance hint; safe to ignore if the DB is locked.
    }
    try {
      this.db.close()
    } catch {
      // The DB may be locked by another connection; the OS will reclaim it on process exit.
    }
  }
}
