import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'

const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

const SQLITE_BUSY = 5
const RETRY_DELAY_MS = 50
const MAX_RETRIES = 100

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

export interface MetadataHeaders {
  etag?: string
  modified?: string
}

export interface MetadataRow {
  data: string
  etag?: string
  modified?: string
  cachedAt?: number
  isFull: boolean
}

const openInstances = new Set<MetadataCache>()

export function closeAllMetadataCaches (): void {
  for (const mc of openInstances) {
    mc.close()
  }
}

interface PendingWrite {
  name: string
  etag: string | null
  modified: string | null
  cachedAt: number
  isFull: boolean
  data: string
}

export class MetadataCache {
  private db: DatabaseSyncType
  private closed = false
  private pendingWrites: PendingWrite[] = []
  private flushScheduled = false
  private stmtGetHeaders: StatementSync
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtDelete: StatementSync
  private stmtListNames: StatementSync
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
          name TEXT PRIMARY KEY,
          etag TEXT,
          modified TEXT,
          cached_at INTEGER,
          is_full INTEGER NOT NULL DEFAULT 0,
          data TEXT NOT NULL
        ) WITHOUT ROWID
      `)
      // Drop tables from previous schema versions
      this.db.exec('DROP TABLE IF EXISTS metadata_index')
      this.db.exec('DROP TABLE IF EXISTS metadata_blobs')
      this.db.exec('DROP TABLE IF EXISTS metadata_manifests')
    })
    this.stmtGetHeaders = this.db.prepare(
      'SELECT etag, modified FROM metadata WHERE name = ?'
    )
    this.stmtGet = this.db.prepare(
      'SELECT data, etag, modified, cached_at, is_full FROM metadata WHERE name = ?'
    )
    this.stmtSet = this.db.prepare(
      'INSERT OR REPLACE INTO metadata (name, etag, modified, cached_at, is_full, data) VALUES (?, ?, ?, ?, ?, ?)'
    )
    this.stmtDelete = this.db.prepare('DELETE FROM metadata WHERE name = ?')
    this.stmtListNames = this.db.prepare('SELECT name FROM metadata')
    this.stmtUpdateCachedAt = this.db.prepare('UPDATE metadata SET cached_at = ? WHERE name = ?')
    this.exitHandler = () => this.close()
    const currentMax = process.getMaxListeners()
    if (currentMax !== 0 && currentMax < openInstances.size + 11) {
      process.setMaxListeners(Math.max(currentMax + 10, openInstances.size + 11))
    }
    process.on('exit', this.exitHandler)
    openInstances.add(this)
  }

  private findPending (name: string): PendingWrite | undefined {
    for (let i = this.pendingWrites.length - 1; i >= 0; i--) {
      if (this.pendingWrites[i].name === name) return this.pendingWrites[i]
    }
    return undefined
  }

  getHeaders (name: string): MetadataHeaders | undefined {
    if (this.closed) return undefined
    const pending = this.findPending(name)
    if (pending) {
      return {
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
      }
    }
    const row = sqliteRetry(() => this.stmtGetHeaders.get(name)) as { etag: string | null, modified: string | null } | undefined
    if (!row) return undefined
    return {
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
    }
  }

  get (name: string): MetadataRow | null {
    if (this.closed) return null
    const pending = this.findPending(name)
    if (pending) {
      return {
        data: pending.data,
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
        cachedAt: pending.cachedAt,
        isFull: pending.isFull,
      }
    }
    const row = sqliteRetry(() => this.stmtGet.get(name)) as {
      data: string
      etag: string | null
      modified: string | null
      cached_at: number | null
      is_full: number
    } | undefined
    if (!row) return null
    return {
      data: row.data,
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
      cachedAt: row.cached_at ?? undefined,
      isFull: row.is_full === 1,
    }
  }

  queueSet (
    name: string,
    data: string,
    opts: { etag?: string, modified?: string, cachedAt: number, isFull?: boolean }
  ): void {
    this.pendingWrites.push({
      name,
      etag: opts.etag ?? null,
      modified: opts.modified ?? null,
      cachedAt: opts.cachedAt,
      isFull: opts.isFull ?? false,
      data,
    })
    if (!this.flushScheduled) {
      this.flushScheduled = true
      process.nextTick(() => this.flush())
    }
  }

  updateCachedAt (name: string, cachedAt: number): void {
    if (this.closed) return
    sqliteRetry(() => {
      this.stmtUpdateCachedAt.run(cachedAt, name)
    })
  }

  flush (): void {
    this.flushScheduled = false
    if (this.pendingWrites.length === 0 || this.closed) return
    const writes = this.pendingWrites
    this.pendingWrites = []
    sqliteRetry(() => {
      if (writes.length === 1) {
        const w = writes[0]
        this.stmtSet.run(w.name, w.etag, w.modified, w.cachedAt, w.isFull ? 1 : 0, w.data)
        return
      }
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const w of writes) {
          this.stmtSet.run(w.name, w.etag, w.modified, w.cachedAt, w.isFull ? 1 : 0, w.data)
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

  delete (name: string): boolean {
    let result!: { changes: number | bigint }
    sqliteRetry(() => {
      result = this.stmtDelete.run(name)
    })
    return result.changes > 0
  }

  listNames (): string[] {
    const rows = sqliteRetry(() => this.stmtListNames.all())
    return (rows as Array<{ name: string }>).map((r) => r.name)
  }

  close (): void {
    if (this.closed) return
    this.flush()
    this.closed = true
    openInstances.delete(this)
    process.removeListener('exit', this.exitHandler)
    try {
      this.db.exec('PRAGMA optimize')
    } catch {}
    try {
      this.db.close()
    } catch {}
  }
}
