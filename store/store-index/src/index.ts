import { createRequire } from 'module'
import path from 'path'
import fs from 'fs'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { Packr } from 'msgpackr'

// Use createRequire to load node:sqlite because it is a prefix-only builtin
// that Jest's ESM module resolver cannot handle.
const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

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
// Exported packForStorage is kept for generic use or fallback without structures.
// Within workers and StoreIndex, passing a synced Packr instance is preferred.
const globalPackr = new Packr({
  useRecords: true,
  moreTypes: true,
})
export function packForStorage (data: unknown): Uint8Array {
  return globalPackr.pack(data)
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
  private stmtSetStructure: StatementSync

  public packr: Packr
  public structures: unknown[]
  private savedStructuresCount: number = 0

  constructor (storeDir: string) {
    const dbPath = path.join(storeDir, 'index.db')
    fs.mkdirSync(storeDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
    })
    // In WAL mode, synchronous=NORMAL is safe but synchronous=OFF provides a massive speedup
    // for bulk inserts. Since pnpm can always re-fetch a corrupted index, OFF is preferred.
    this.db.exec('PRAGMA synchronous=OFF')
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
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS msgpack_structures (
          id INTEGER PRIMARY KEY,
          data BLOB NOT NULL
        );
      `)
    })

    const stmtGetStructures = this.db.prepare('SELECT id, data FROM msgpack_structures ORDER BY id ASC')
    const rows = stmtGetStructures.all() as Array<{id: number, data: Uint8Array}>
    const globalUnpacker = new Packr({ useRecords: true, moreTypes: true })
    this.structures = rows.map(r => globalUnpacker.unpack(r.data))

    if (this.structures.length === 0) {
      this.structures = [
        ['requiresBuild', 'manifest', 'algo', 'files'],
        ['checkedAt', 'digest', 'mode', 'size'],
        ['name', 'version', 'dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta', 'bin', 'scripts'],
      ]
    }

    this.savedStructuresCount = rows.length

    this.packr = new Packr({
      useRecords: true,
      moreTypes: true,
      // eslint-disable-next-line @typescript-eslint/no-empty-object-type
      structures: this.structures as {}[],
      maxSharedStructures: this.structures.length, // Freeze structures to prevent cross-worker divergence
    })

    this.stmtGet = this.db.prepare('SELECT data FROM package_index WHERE key = ?')
    this.stmtSet = this.db.prepare('INSERT OR REPLACE INTO package_index (key, data) VALUES (?, ?)')
    this.stmtDel = this.db.prepare('DELETE FROM package_index WHERE key = ?')
    this.stmtHas = this.db.prepare('SELECT 1 FROM package_index WHERE key = ?')
    this.stmtAll = this.db.prepare('SELECT key, data FROM package_index')
    this.stmtSetStructure = this.db.prepare('INSERT OR REPLACE INTO msgpack_structures (id, data) VALUES (?, ?)')

    this.saveNewStructures()
  }

  saveNewStructures (): void {
    if (this.structures.length > this.savedStructuresCount) {
      sqliteRetry(() => {
        this.db.exec('BEGIN IMMEDIATE')
        let committed = false
        try {
          for (let i = this.savedStructuresCount; i < this.structures.length; i++) {
            const buffer = globalPackr.pack(this.structures[i])
            this.stmtSetStructure.run(i, buffer)
          }
          this.db.exec('COMMIT')
          committed = true
          this.savedStructuresCount = this.structures.length
        } finally {
          if (!committed) {
            try {
              this.db.exec('ROLLBACK')
            } catch {}
          }
        }
      })
    }
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
        return this.packr.unpack(row.data)
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
      const buffer = this.packr.pack(data)
      sqliteRetry(() => {
        this.stmtSet.run(key, buffer)
      })
      this.saveNewStructures()
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
      yield [row.key, this.packr.unpack(row.data)]
    }
  }

  /**
   * Write multiple pre-packed entries in a single transaction.
   * The buffers must already be msgpack-encoded.
   */
  setRawMany (entries: Array<{ key: string, buffer: Uint8Array }>): void {
    if (entries.length === 0) return
    if (entries.length === 1) {
      sqliteRetry(() => {
        this.stmtSet.run(entries[0].key, entries[0].buffer)
      })
      this.saveNewStructures()
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
        // Important: After a successful transaction, save any newly discovered structures!
        this.saveNewStructures()
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
    return globalPackr.unpack(buffer)
  } catch {
    return undefined
  }
}

function writeMpkFileSync (filePath: string, data: unknown): void {
  const targetDir = path.dirname(filePath)
  fs.mkdirSync(targetDir, { recursive: true })
  const buffer = globalPackr.pack(data)
  // Atomic write: write to temp file, then rename
  const temp = `${filePath}.${process.pid}.tmp`
  fs.writeFileSync(temp, buffer)
  fs.renameSync(temp, filePath)
}
