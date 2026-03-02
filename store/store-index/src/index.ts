import { createRequire } from 'module'
import path from 'path'
import fs from 'fs'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { Packr } from 'msgpackr'

// Use createRequire to load node:sqlite because it is a prefix-only builtin
// that Jest's ESM module resolver cannot handle.
const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

/**
 * Use the same msgpackr configuration as @pnpm/fs.msgpack-file
 * to ensure compatibility with existing .mpk files.
 */
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

export class StoreIndex {
  private db: DatabaseSyncType
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtDel: StatementSync
  private stmtAll: StatementSync
  private indexDir: string
  private indexPrefix: string

  constructor (storeDir: string) {
    this.indexDir = path.join(storeDir, 'index')
    this.indexPrefix = this.indexDir + path.sep
    const dbPath = path.join(storeDir, 'index.db')
    fs.mkdirSync(storeDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
    // DatabaseSync does not honor busy_timeout, so we use manual retries
    // for statements that require write locks (WAL setup, CREATE TABLE).
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
    })
    this.db.exec('PRAGMA synchronous=NORMAL')
    this.db.exec('PRAGMA mmap_size=268435456')
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
    this.stmtAll = this.db.prepare('SELECT key, data FROM package_index')
  }

  /**
   * Derives a SQLite key from a full filesIndexFile path.
   * Returns null if the path is not inside the index/ directory
   * (e.g. integrity.mpk files stored inside package directories).
   */
  pathToKey (filePath: string): string | null {
    if (!filePath.startsWith(this.indexPrefix)) return null
    // Strip the index/ prefix and .mpk suffix
    const relative = filePath.slice(this.indexPrefix.length)
    if (relative.endsWith('.mpk')) {
      return relative.slice(0, -4)
    }
    return relative
  }

  /**
   * Read a PackageFilesIndex from the store.
   * Tries SQLite first, then falls back to reading the .mpk file.
   */
  get (filesIndexFile: string): unknown | undefined {
    const key = this.pathToKey(filesIndexFile)
    if (key == null) {
      // Not in index/ directory — read .mpk file directly
      return readMpkFileSync(filesIndexFile)
    }
    const row = this.stmtGet.get(key) as { data: Uint8Array } | undefined
    if (row) {
      return packr.unpack(row.data)
    }
    // Fall back to reading legacy .mpk file
    const data = readMpkFileSync(filesIndexFile)
    if (data != null) {
      // Migrate to SQLite on read
      sqliteRetry(() => {
        this.stmtSet.run(key, packr.pack(data))
      })
    }
    return data
  }

  /**
   * Write a PackageFilesIndex to the store.
   * Always writes to SQLite. For paths outside index/, writes .mpk file.
   */
  set (filesIndexFile: string, data: unknown): void {
    const key = this.pathToKey(filesIndexFile)
    if (key == null) {
      // Not in index/ directory — write .mpk file directly
      writeMpkFileSync(filesIndexFile, data)
      return
    }
    const buffer = packr.pack(data)
    sqliteRetry(() => {
      this.stmtSet.run(key, buffer)
    })
  }

  /**
   * Delete an index entry.
   */
  delete (filesIndexFile: string): boolean {
    const key = this.pathToKey(filesIndexFile)
    if (key == null) {
      try {
        fs.unlinkSync(filesIndexFile)
        return true
      } catch {
        return false
      }
    }
    let result!: { changes: number | bigint }
    sqliteRetry(() => {
      result = this.stmtDel.run(key)
    })
    // Also try to delete legacy .mpk file
    try {
      fs.unlinkSync(filesIndexFile)
    } catch {
      // ignore
    }
    return result.changes > 0
  }

  /**
   * Iterate over all index entries.
   * Yields [filesIndexFile, data] pairs.
   * Includes both SQLite entries and legacy .mpk files not yet migrated.
   */
  * entries (): IterableIterator<[string, unknown]> {
    const seenKeys = new Set<string>()
    // First, yield all entries from SQLite
    const rows = this.stmtAll.all() as Array<{ key: string, data: Uint8Array }>
    for (const row of rows) {
      seenKeys.add(row.key)
      const filesIndexFile = path.join(this.indexDir, `${row.key}.mpk`)
      const data = packr.unpack(row.data)
      yield [filesIndexFile, data]
    }
    // Then, scan the index/ directory for legacy .mpk files
    yield * this._scanLegacyFiles(seenKeys)
  }

  private * _scanLegacyFiles (seenKeys: Set<string>): IterableIterator<[string, unknown]> {
    let subdirs: string[]
    try {
      subdirs = fs.readdirSync(this.indexDir)
    } catch {
      return
    }
    for (const subdir of subdirs) {
      const subdirPath = path.join(this.indexDir, subdir)
      let stat: fs.Stats
      try {
        stat = fs.statSync(subdirPath)
      } catch {
        continue
      }
      if (!stat.isDirectory()) continue
      let files: string[]
      try {
        files = fs.readdirSync(subdirPath)
      } catch {
        continue
      }
      for (const file of files) {
        if (!file.endsWith('.mpk')) continue
        const key = path.join(subdir, file.slice(0, -4))
        if (seenKeys.has(key)) continue
        const filePath = path.join(subdirPath, file)
        const data = readMpkFileSync(filePath)
        if (data != null) {
          // Migrate to SQLite on scan
          sqliteRetry(() => {
            this.stmtSet.run(key, packr.pack(data))
          })
          yield [filePath, data]
        }
      }
    }
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
