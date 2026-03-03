import { createRequire } from 'module'
import path from 'path'
import fs from 'fs'
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

function sqliteRetry (fn: () => void): void {
  for (let attempt = 0; ; attempt++) {
    try {
      fn()
      return
    } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
      if (err?.errcode === SQLITE_BUSY && attempt < MAX_RETRIES) {
        sleepSync(RETRY_DELAY_MS)
        continue
      }
      throw err
    }
  }
}

function sleepSync (ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

/**
 * Create a store index key from an integrity hash and package id.
 * The key is `${integrity}\t${pkgId}` — tab-separated.
 * Integrity strings never contain tabs, so this is unambiguous.
 */
/**
 * Pack data for storage using msgpackr.
 * Use this when data will be packed in one thread and stored by another,
 * to ensure the same Packr instance is used for pack and unpack within each thread.
 */
export function packForStorage (data: unknown): Uint8Array {
  return packr.pack(data)
}

export function storeIndexKey (integrity: string, pkgId: string): string {
  return `${integrity}\t${pkgId}`
}

function isSqliteKey (key: string): boolean {
  return key.includes('\t')
}

export class StoreIndex {
  private db: DatabaseSyncType
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtDel: StatementSync
  private stmtHas: StatementSync
  private stmtAll: StatementSync

  constructor (storeDir: string) {
    const dbPath = path.join(storeDir, 'index.db')
    fs.mkdirSync(storeDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
    })
    this.db.exec('PRAGMA synchronous=NORMAL')
    // Increase memory map size to 512MB
    this.db.exec('PRAGMA mmap_size=536870912')
    // Increase page cache size to ~32MB
    this.db.exec('PRAGMA cache_size=-32000')
    this.db.exec('PRAGMA temp_store=MEMORY')
    // Increase wal autocheckpoint interval to reduce I/O during heavy writes
    this.db.exec('PRAGMA wal_autocheckpoint=10000')
    sqliteRetry(() => {
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
  }

  /**
   * Read a PackageFilesIndex from the store.
   * Keys containing \t are SQLite keys (integrity\tpkgId).
   * Other keys are treated as .mpk file paths.
   */
  get (key: string): unknown | undefined {
    if (isSqliteKey(key)) {
      const row = this.stmtGet.get(key) as { data: Uint8Array } | undefined
      if (row) {
        return packr.unpack(row.data)
      }
      return undefined
    }
    return readMpkFileSync(key)
  }

  /**
   * Write a PackageFilesIndex to the store.
   * Keys containing \t are written to SQLite.
   * Other keys are written as .mpk files.
   */
  set (key: string, data: unknown): void {
    if (isSqliteKey(key)) {
      const buffer = packr.pack(data)
      sqliteRetry(() => {
        this.stmtSet.run(key, buffer)
      })
      return
    }
    writeMpkFileSync(key, data)
  }

  /**
   * Delete an index entry.
   */
  delete (key: string): boolean {
    if (isSqliteKey(key)) {
      let result!: { changes: number | bigint }
      sqliteRetry(() => {
        result = this.stmtDel.run(key)
      })
      return result.changes > 0
    }
    try {
      fs.unlinkSync(key)
      return true
    } catch {
      return false
    }
  }

  /**
   * Check if an index entry exists.
   */
  has (key: string): boolean {
    if (isSqliteKey(key)) {
      return this.stmtHas.get(key) != null
    }
    return fs.existsSync(key)
  }

  /**
   * Iterate over all SQLite index entries.
   * Yields [key, data] pairs where key is `integrity\tpkgId`.
   */
  * entries (): IterableIterator<[string, unknown]> {
    for (const row of this.stmtAll.iterate() as IterableIterator<{ key: string, data: Uint8Array }>) {
      yield [row.key, packr.unpack(row.data)]
    }
  }

  /**
   * Write multiple pre-packed entries in a single transaction.
   * The buffers must already be msgpack-encoded.
   * Keys containing \t are written to SQLite; other keys are written as .mpk files.
   */
  setRawMany (entries: Array<{ key: string, buffer: Uint8Array }>): void {
    if (entries.length === 0) return
    const sqliteEntries: Array<{ key: string, buffer: Uint8Array }> = []
    for (const entry of entries) {
      if (isSqliteKey(entry.key)) {
        sqliteEntries.push(entry)
      } else {
        writeRawMpkFileSync(entry.key, entry.buffer)
      }
    }
    if (sqliteEntries.length === 0) return
    if (sqliteEntries.length === 1) {
      sqliteRetry(() => {
        this.stmtSet.run(sqliteEntries[0].key, sqliteEntries[0].buffer)
      })
      return
    }
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const { key, buffer } of sqliteEntries) {
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
   * Delete multiple index entries in a single transaction.
   */
  deleteMany (keys: string[]): void {
    if (keys.length === 0) return
    if (keys.length === 1) {
      this.delete(keys[0])
      return
    }
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const key of keys) {
          if (isSqliteKey(key)) {
            this.stmtDel.run(key)
          } else {
            try {
              fs.unlinkSync(key)
            } catch {}
          }
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

  close (): void {
    this.db.close()
  }
}

function readMpkFileSync (filePath: string): unknown | undefined {
  try {
    const buffer = fs.readFileSync(filePath)
    return packr.unpack(buffer)
  } catch {
    return undefined
  }
}

function writeMpkFileSync (filePath: string, data: unknown): void {
  const targetDir = path.dirname(filePath)
  fs.mkdirSync(targetDir, { recursive: true })
  const buffer = packr.pack(data)
  // Atomic write: write to temp file, then rename
  const temp = `${filePath}.${process.pid}.tmp`
  fs.writeFileSync(temp, buffer)
  fs.renameSync(temp, filePath)
}

function writeRawMpkFileSync (filePath: string, buffer: Uint8Array): void {
  const targetDir = path.dirname(filePath)
  fs.mkdirSync(targetDir, { recursive: true })
  const temp = `${filePath}.${process.pid}.tmp`
  fs.writeFileSync(temp, buffer)
  fs.renameSync(temp, filePath)
}
