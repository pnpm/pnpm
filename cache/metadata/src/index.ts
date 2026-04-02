import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'

const req = createRequire(import.meta.url)
const { DatabaseSync } = req('node:sqlite') as { DatabaseSync: typeof DatabaseSyncType }

export interface MetadataHeaders {
  etag?: string
  modified?: string
}

export interface MetadataRow {
  data: string | Uint8Array
  etag?: string
  modified?: string
  cachedAt?: number
  isFull: boolean
}

interface HeaderEntry {
  etag?: string
  modified?: string
  cachedAt?: number
  isFull: boolean
}

interface PendingWrite {
  name: string
  etag: string | null
  modified: string | null
  cachedAt: number
  isFull: boolean
  data: string | Uint8Array
}

interface DbState {
  db: DatabaseSyncType
  stmtGet: StatementSync
  stmtGetHeaders: StatementSync
  stmtSet: StatementSync
  stmtDelete: StatementSync
  stmtListNames: StatementSync
  stmtUpdateCachedAt: StatementSync
  pendingWrites: PendingWrite[]
  pendingByName: Map<string, PendingWrite>
  pendingCachedAtUpdates: Map<string, number>
  flushScheduled: boolean
  headerCache: Map<string, HeaderEntry | null>
}

const dbStates = new Map<string, DbState>()

function getDbState (cacheDir: string): DbState {
  const resolvedDir = path.resolve(cacheDir)
  let state = dbStates.get(resolvedDir)
  if (state) return state

  const dbPath = path.join(resolvedDir, 'metadata.db')
  fs.mkdirSync(resolvedDir, { recursive: true })
  const dbExists = fs.existsSync(dbPath)
  const db = new DatabaseSync(dbPath)

  // High-performance PRAGMAs
  db.exec('PRAGMA busy_timeout=5000')
  if (!dbExists) {
    db.exec('PRAGMA page_size=16384')
  }
  db.exec('PRAGMA journal_mode=WAL')
  db.exec('PRAGMA synchronous=OFF')
  db.exec('PRAGMA locking_mode=EXCLUSIVE') // Drastically speeds up single-process access
  db.exec('PRAGMA mmap_size=536870912')
  db.exec('PRAGMA cache_size=-64000')
  db.exec('PRAGMA temp_store=MEMORY')
  db.exec('PRAGMA wal_autocheckpoint=10000')
  db.exec('PRAGMA journal_size_limit=67108864')

  db.exec(`
    CREATE TABLE IF NOT EXISTS metadata (
      name TEXT PRIMARY KEY,
      etag TEXT,
      modified TEXT,
      cached_at INTEGER,
      is_full INTEGER NOT NULL DEFAULT 0,
      data TEXT NOT NULL
    )
  `)
  db.exec('CREATE INDEX IF NOT EXISTS idx_metadata_headers ON metadata (name, etag, modified)')

  state = {
    db,
    stmtGet: db.prepare('SELECT data, etag, modified, cached_at, is_full FROM metadata WHERE name = ?'),
    stmtGetHeaders: db.prepare('SELECT etag, modified, cached_at, is_full FROM metadata WHERE name = ?'),
    stmtSet: db.prepare('INSERT OR REPLACE INTO metadata (name, etag, modified, cached_at, is_full, data) VALUES (?, ?, ?, ?, ?, ?)'),
    stmtDelete: db.prepare('DELETE FROM metadata WHERE name = ?'),
    stmtListNames: db.prepare('SELECT name FROM metadata'),
    stmtUpdateCachedAt: db.prepare('UPDATE metadata SET cached_at = ? WHERE name = ?'),
    pendingWrites: [],
    pendingByName: new Map(),
    pendingCachedAtUpdates: new Map(),
    flushScheduled: false,
    headerCache: new Map(), // Lazy populated
  }
  dbStates.set(resolvedDir, state)
  return state
}

function flushState (state: DbState): void {
  state.flushScheduled = false
  if (state.pendingWrites.length === 0 && state.pendingCachedAtUpdates.size === 0) return

  const writes = state.pendingWrites
  state.pendingWrites = []
  state.pendingByName.clear()

  const updates = Array.from(state.pendingCachedAtUpdates.entries())
  state.pendingCachedAtUpdates.clear()

  if (writes.length + updates.length === 1) {
    try {
      if (writes.length === 1) {
        const w = writes[0]
        state.stmtSet.run(w.name, w.etag, w.modified, w.cachedAt, w.isFull ? 1 : 0, w.data)
      } else {
        const [name, cachedAt] = updates[0]
        state.stmtUpdateCachedAt.run(cachedAt, name)
      }
      return
    } catch (_err) {
      // ignore
    }
  }

  try {
    state.db.exec('BEGIN IMMEDIATE')
    let committed = false
    try {
      for (const w of writes) {
        state.stmtSet.run(w.name, w.etag, w.modified, w.cachedAt, w.isFull ? 1 : 0, w.data)
      }
      for (const [name, cachedAt] of updates) {
        state.stmtUpdateCachedAt.run(cachedAt, name)
      }
      state.db.exec('COMMIT')
      committed = true
    } finally {
      if (!committed) {
        try {
          state.db.exec('ROLLBACK')
        } catch {}
      }
    }
  } catch (_err) {
    // ignore
  }
}

export function closeAllMetadataCaches (): void {
  for (const state of dbStates.values()) {
    flushState(state)
    try {
      state.db.exec('PRAGMA optimize')
      state.db.close()
    } catch {}
  }
  dbStates.clear()
}

process.on('exit', () => {
  for (const state of dbStates.values()) {
    flushState(state)
    try {
      state.db.close()
    } catch {}
  }
})

export class MetadataCache {
  private state: DbState

  constructor (cacheDir: string) {
    this.state = getDbState(cacheDir)
  }

  private getHeader (name: string): HeaderEntry | undefined {
    const cached = this.state.headerCache.get(name)
    if (cached !== undefined) return cached ?? undefined

    const row = this.state.stmtGetHeaders.get(name) as {
      etag: string | null
      modified: string | null
      cached_at: number | null
      is_full: number
    } | undefined

    if (!row) {
      this.state.headerCache.set(name, null)
      return undefined
    }

    const entry: HeaderEntry = {
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
      cachedAt: row.cached_at ?? undefined,
      isFull: row.is_full === 1,
    }
    this.state.headerCache.set(name, entry)
    return entry
  }

  getHeaders (name: string): MetadataHeaders | undefined {
    const pending = this.state.pendingByName.get(name)
    if (pending) {
      return {
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
      }
    }
    const header = this.getHeader(name)
    if (!header) return undefined
    return {
      etag: header.etag,
      modified: header.modified,
    }
  }

  get (name: string): MetadataRow | null {
    const pending = this.state.pendingByName.get(name)
    if (pending) {
      return {
        data: pending.data,
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
        cachedAt: pending.cachedAt,
        isFull: pending.isFull,
      }
    }

    // Check if we even have this entry before doing a full read
    const header = this.getHeader(name)
    if (!header) return null

    const row = this.state.stmtGet.get(name) as {
      data: string | Uint8Array
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
      cachedAt: this.state.pendingCachedAtUpdates.get(name) ?? row.cached_at ?? undefined,
      isFull: row.is_full === 1,
    }
  }

  queueSet (
    name: string,
    data: string | Uint8Array,
    opts: { etag?: string, modified?: string, cachedAt: number, isFull?: boolean }
  ): void {
    const entry: PendingWrite = {
      name,
      etag: opts.etag ?? null,
      modified: opts.modified ?? null,
      cachedAt: opts.cachedAt,
      isFull: opts.isFull ?? false,
      data,
    }
    this.state.pendingWrites.push(entry)
    this.state.pendingByName.set(name, entry)
    this.state.pendingCachedAtUpdates.delete(name)

    // Update header cache
    this.state.headerCache.set(name, {
      etag: opts.etag,
      modified: opts.modified,
      cachedAt: opts.cachedAt,
      isFull: opts.isFull ?? false,
    })

    this.scheduleFlush()
  }

  updateCachedAt (name: string, cachedAt: number): void {
    const pending = this.state.pendingByName.get(name)
    if (pending) {
      pending.cachedAt = cachedAt
    } else {
      this.state.pendingCachedAtUpdates.set(name, cachedAt)
    }

    // Update header cache
    const header = this.state.headerCache.get(name)
    if (header) {
      header.cachedAt = cachedAt
    }

    this.scheduleFlush()
  }

  private scheduleFlush (): void {
    if (!this.state.flushScheduled) {
      this.state.flushScheduled = true
      process.nextTick(() => flushState(this.state))
    }
  }

  flush (): void {
    flushState(this.state)
  }

  delete (name: string): boolean {
    const result = this.state.stmtDelete.run(name)
    this.state.headerCache.set(name, null)
    return result.changes > 0
  }

  listNames (): string[] {
    const rows = this.state.stmtListNames.all() as Array<{ name: string }>
    return rows.map((r) => r.name)
  }

  close (): void {
    this.flush()
  }
}
