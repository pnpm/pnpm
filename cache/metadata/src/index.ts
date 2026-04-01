import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'

// Use createRequire to load node:sqlite because it is a prefix-only builtin
// that Jest's ESM module resolver cannot handle.
const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

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
  return (err?.errcode & 0xFF) === SQLITE_BUSY
}

const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))

function sleepSync (ms: number): void {
  Atomics.wait(sleepBuffer, 0, 0, ms)
}

export type MetadataType = 'abbreviated' | 'full' | 'full-filtered'

export interface MetadataHeaders {
  etag?: string
  modified?: string
}

export interface MetadataRow {
  data: string
  etag?: string
  modified?: string
  cachedAt?: number
}

const openInstances = new Set<MetadataCache>()

/**
 * Close all open MetadataCache instances.
 * Useful in tests that need to remove the cache directory.
 */
export function closeAllMetadataCaches (): void {
  for (const mc of openInstances) {
    mc.close()
  }
}

export class MetadataCache {
  private db: DatabaseSyncType
  private closed = false
  private stmtGetHeaders: StatementSync
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtDeleteName: StatementSync
  private stmtDeleteNameType: StatementSync
  private stmtListNames: StatementSync
  private stmtListNamesByType: StatementSync
  private stmtUpdateCachedAt: StatementSync
  private readonly exitHandler: () => void

  constructor (cacheDir: string) {
    const dbPath = path.join(cacheDir, 'metadata.db')
    fs.mkdirSync(cacheDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
    this.db.exec('PRAGMA busy_timeout=5000')
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
      this.db.exec('PRAGMA synchronous=NORMAL')
      this.db.exec('PRAGMA mmap_size=536870912')
      this.db.exec('PRAGMA cache_size=-32000')
      this.db.exec('PRAGMA temp_store=MEMORY')
      this.db.exec('PRAGMA wal_autocheckpoint=10000')
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS metadata (
          name TEXT NOT NULL,
          type TEXT NOT NULL,
          etag TEXT,
          modified TEXT,
          cached_at INTEGER,
          data TEXT NOT NULL,
          PRIMARY KEY (name, type)
        ) WITHOUT ROWID
      `)
    })
    this.stmtGetHeaders = this.db.prepare(
      'SELECT etag, modified FROM metadata WHERE name = ? AND type = ?'
    )
    this.stmtGet = this.db.prepare(
      'SELECT data, etag, modified, cached_at FROM metadata WHERE name = ? AND type = ?'
    )
    this.stmtSet = this.db.prepare(
      'INSERT OR REPLACE INTO metadata (name, type, etag, modified, cached_at, data) VALUES (?, ?, ?, ?, ?, ?)'
    )
    this.stmtDeleteName = this.db.prepare('DELETE FROM metadata WHERE name = ?')
    this.stmtDeleteNameType = this.db.prepare('DELETE FROM metadata WHERE name = ? AND type = ?')
    this.stmtListNames = this.db.prepare('SELECT DISTINCT name FROM metadata')
    this.stmtListNamesByType = this.db.prepare('SELECT DISTINCT name FROM metadata WHERE type = ?')
    this.stmtUpdateCachedAt = this.db.prepare('UPDATE metadata SET cached_at = ? WHERE name = ? AND type = ?')
    this.exitHandler = () => this.close()
    const currentMax = process.getMaxListeners()
    if (currentMax !== 0 && currentMax < openInstances.size + 11) {
      process.setMaxListeners(Math.max(currentMax + 10, openInstances.size + 11))
    }
    process.on('exit', this.exitHandler)
    openInstances.add(this)
  }

  /**
   * Get only the conditional-request headers for a package.
   * This is cheap — no JSON parsing.
   * Falls back from abbreviated → full-filtered → full.
   */
  getHeaders (name: string, type: MetadataType): MetadataHeaders | undefined {
    let row = sqliteRetry(() => this.stmtGetHeaders.get(name, type)) as { etag: string | null, modified: string | null } | undefined
    if (!row && type === 'abbreviated') {
      row = sqliteRetry(() => this.stmtGetHeaders.get(name, 'full-filtered')) as typeof row
      if (!row) {
        row = sqliteRetry(() => this.stmtGetHeaders.get(name, 'full')) as typeof row
      }
    }
    if (!row) return undefined
    return {
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
    }
  }

  /**
   * Get full metadata for a package.
   * Falls back from abbreviated → full-filtered → full.
   */
  get (name: string, type: MetadataType): MetadataRow | null {
    let row = sqliteRetry(() => this.stmtGet.get(name, type)) as {
      data: string
      etag: string | null
      modified: string | null
      cached_at: number | null
    } | undefined
    if (!row && type === 'abbreviated') {
      row = sqliteRetry(() => this.stmtGet.get(name, 'full-filtered')) as typeof row
      if (!row) {
        row = sqliteRetry(() => this.stmtGet.get(name, 'full')) as typeof row
      }
    }
    if (!row) return null
    return {
      data: row.data,
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
      cachedAt: row.cached_at ?? undefined,
    }
  }

  /**
   * Store metadata for a package.
   */
  set (
    name: string,
    type: MetadataType,
    data: string,
    opts: { etag?: string, modified?: string, cachedAt: number }
  ): void {
    sqliteRetry(() => {
      this.stmtSet.run(name, type, opts.etag ?? null, opts.modified ?? null, opts.cachedAt, data)
    })
  }

  /**
   * Update cachedAt without rewriting the data.
   * Falls back from abbreviated → full-filtered → full.
   */
  updateCachedAt (name: string, type: MetadataType, cachedAt: number): void {
    sqliteRetry(() => {
      let result = this.stmtUpdateCachedAt.run(cachedAt, name, type)
      if (result.changes === 0 && type === 'abbreviated') {
        result = this.stmtUpdateCachedAt.run(cachedAt, name, 'full-filtered')
        if (result.changes === 0) {
          this.stmtUpdateCachedAt.run(cachedAt, name, 'full')
        }
      }
    })
  }

  /**
   * Delete all metadata for a package name.
   */
  delete (name: string): boolean {
    let result!: { changes: number | bigint }
    sqliteRetry(() => {
      result = this.stmtDeleteName.run(name)
    })
    return result.changes > 0
  }

  /**
   * Delete metadata for a specific package name and type.
   */
  deleteByType (name: string, type: MetadataType): boolean {
    let result!: { changes: number | bigint }
    sqliteRetry(() => {
      result = this.stmtDeleteNameType.run(name, type)
    })
    return result.changes > 0
  }

  /**
   * List all distinct package names, optionally filtered by type.
   */
  listNames (type?: MetadataType): string[] {
    const rows = type
      ? sqliteRetry(() => this.stmtListNamesByType.all(type))
      : sqliteRetry(() => this.stmtListNames.all())
    return (rows as Array<{ name: string }>).map((r) => r.name)
  }

  close (): void {
    if (this.closed) return
    this.closed = true
    openInstances.delete(this)
    process.removeListener('exit', this.exitHandler)
    try {
      this.db.exec('PRAGMA optimize')
    } catch {
      // Safe to ignore if the DB is locked.
    }
    try {
      this.db.close()
    } catch {
      // The OS will reclaim it on process exit.
    }
  }
}
