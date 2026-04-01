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

export interface MetadataIndex {
  distTags: string
  versions: string
  time?: string
  etag?: string
  modified?: string
  cachedAt?: number
  isFull?: boolean
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
  distTags: string
  versions: string
  time: string | null
  blob: string
}

export class MetadataCache {
  private db: DatabaseSyncType
  private closed = false
  private pendingWrites: PendingWrite[] = []
  private flushScheduled = false
  private stmtGetIndex: StatementSync
  private stmtGetHeaders: StatementSync
  private stmtSetIndex: StatementSync
  private stmtGetBlob: StatementSync
  private stmtSetBlob: StatementSync
  private stmtDeleteIndex: StatementSync
  private stmtDeleteBlob: StatementSync
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
        CREATE TABLE IF NOT EXISTS metadata_index (
          name TEXT PRIMARY KEY,
          etag TEXT,
          modified TEXT,
          cached_at INTEGER,
          is_full INTEGER NOT NULL DEFAULT 0,
          dist_tags TEXT NOT NULL,
          versions TEXT NOT NULL,
          time TEXT
        ) WITHOUT ROWID
      `)
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS metadata_blobs (
          name TEXT PRIMARY KEY,
          data TEXT NOT NULL
        ) WITHOUT ROWID
      `)
      // Drop old per-version manifests table if it exists
      this.db.exec('DROP TABLE IF EXISTS metadata_manifests')
      // Drop old single-table design if it exists
      this.db.exec('DROP TABLE IF EXISTS metadata')
    })
    this.stmtGetIndex = this.db.prepare(
      'SELECT dist_tags, versions, time, etag, modified, cached_at, is_full FROM metadata_index WHERE name = ?'
    )
    this.stmtGetHeaders = this.db.prepare(
      'SELECT etag, modified FROM metadata_index WHERE name = ?'
    )
    this.stmtSetIndex = this.db.prepare(
      'INSERT OR REPLACE INTO metadata_index (name, etag, modified, cached_at, is_full, dist_tags, versions, time) VALUES (?, ?, ?, ?, ?, ?, ?, ?)'
    )
    this.stmtGetBlob = this.db.prepare(
      'SELECT data FROM metadata_blobs WHERE name = ?'
    )
    this.stmtSetBlob = this.db.prepare(
      'INSERT OR REPLACE INTO metadata_blobs (name, data) VALUES (?, ?)'
    )
    this.stmtDeleteIndex = this.db.prepare('DELETE FROM metadata_index WHERE name = ?')
    this.stmtDeleteBlob = this.db.prepare('DELETE FROM metadata_blobs WHERE name = ?')
    this.stmtListNames = this.db.prepare('SELECT name FROM metadata_index')
    this.stmtUpdateCachedAt = this.db.prepare('UPDATE metadata_index SET cached_at = ? WHERE name = ?')
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

  getIndex (name: string): MetadataIndex | null {
    if (this.closed) return null
    const pending = this.findPending(name)
    if (pending) {
      return {
        distTags: pending.distTags,
        versions: pending.versions,
        time: pending.time ?? undefined,
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
        cachedAt: pending.cachedAt,
        isFull: pending.isFull,
      }
    }
    const row = sqliteRetry(() => this.stmtGetIndex.get(name)) as {
      dist_tags: string
      versions: string
      time: string | null
      etag: string | null
      modified: string | null
      cached_at: number | null
      is_full: number
    } | undefined
    if (!row) return null
    return {
      distTags: row.dist_tags,
      versions: row.versions,
      time: row.time ?? undefined,
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
      cachedAt: row.cached_at ?? undefined,
      isFull: row.is_full === 1,
    }
  }

  /**
   * Get the raw JSON blob for a package.
   * Used after version picking to extract the resolved version's manifest.
   */
  getBlob (name: string): string | null {
    if (this.closed) return null
    const pending = this.findPending(name)
    if (pending) return pending.blob
    const row = sqliteRetry(() => this.stmtGetBlob.get(name)) as { data: string } | undefined
    return row?.data ?? null
  }

  /**
   * Queue a write. Extracts index fields cheaply from parsed meta,
   * stores the raw JSON blob as-is (zero serialization on hot path).
   */
  queueWrite (
    name: string,
    meta: {
      'dist-tags'?: Record<string, string>
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      versions?: Record<string, any> | null
      time?: Record<string, string> & { unpublished?: unknown }
      modified?: string
    },
    rawJson: string,
    opts: { etag?: string, cachedAt: number, isFull?: boolean }
  ): void {
    if (!meta['dist-tags'] || !meta.versions) return
    const versionsCompact: Record<string, { deprecated?: string }> = {}
    for (const [v, manifest] of Object.entries(meta.versions)) {
      versionsCompact[v] = manifest.deprecated ? { deprecated: manifest.deprecated as string } : {}
    }

    this.pendingWrites.push({
      name,
      etag: opts.etag ?? null,
      modified: meta.modified ?? meta.time?.modified ?? null,
      cachedAt: opts.cachedAt,
      isFull: opts.isFull ?? false,
      distTags: JSON.stringify(meta['dist-tags']),
      versions: JSON.stringify(versionsCompact),
      time: meta.time ? JSON.stringify(meta.time) : null,
      blob: rawJson,
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
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        for (const w of writes) {
          this.stmtSetIndex.run(w.name, w.etag, w.modified, w.cachedAt, w.isFull ? 1 : 0, w.distTags, w.versions, w.time)
          this.stmtSetBlob.run(w.name, w.blob)
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
    let changes!: number | bigint
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        const r1 = this.stmtDeleteIndex.run(name)
        this.stmtDeleteBlob.run(name)
        changes = r1.changes
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
    return changes > 0
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
