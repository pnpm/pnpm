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
  data: string
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
  data: string
}

interface DbState {
  db: DatabaseSyncType
  stmtGet: StatementSync
  stmtSet: StatementSync
  stmtDelete: StatementSync
  stmtListNames: StatementSync
  stmtUpdateCachedAt: StatementSync
  stmtGetMany: StatementSync
  pendingWrites: PendingWrite[]
  pendingByName: Map<string, PendingWrite>
  pendingCachedAtUpdates: Map<string, number>
  flushScheduled: boolean
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
  db.exec('PRAGMA busy_timeout=5000')
  if (!dbExists) {
    db.exec('PRAGMA page_size=16384')
  }
  db.exec('PRAGMA journal_mode=WAL')
  db.exec('PRAGMA synchronous=OFF')
  db.exec('PRAGMA mmap_size=536870912')
  db.exec('PRAGMA cache_size=-64000')
  db.exec('PRAGMA temp_store=MEMORY')
  db.exec(`
    CREATE TABLE IF NOT EXISTS metadata (
      name TEXT PRIMARY KEY,
      etag TEXT,
      modified TEXT,
      cached_at INTEGER,
      is_full INTEGER NOT NULL DEFAULT 0,
      data TEXT NOT NULL
    ) WITHOUT ROWID
  `)
  // Create temporary table for batch lookups
  db.exec('CREATE TEMPORARY TABLE IF NOT EXISTS lookup_keys (name TEXT PRIMARY KEY) WITHOUT ROWID')

  // Drop tables from previous schema versions
  db.exec('DROP TABLE IF EXISTS metadata_index')
  db.exec('DROP TABLE IF EXISTS metadata_blobs')
  db.exec('DROP TABLE IF EXISTS metadata_manifests')
  db.exec('DROP INDEX IF EXISTS idx_metadata_headers')

  state = {
    db,
    stmtGet: db.prepare('SELECT data, etag, modified, cached_at, is_full FROM metadata WHERE name = ?'),
    stmtSet: db.prepare('INSERT OR REPLACE INTO metadata (name, etag, modified, cached_at, is_full, data) VALUES (?, ?, ?, ?, ?, ?)'),
    stmtDelete: db.prepare('DELETE FROM metadata WHERE name = ?'),
    stmtListNames: db.prepare('SELECT name FROM metadata'),
    stmtUpdateCachedAt: db.prepare('UPDATE metadata SET cached_at = ? WHERE name = ?'),
    stmtGetMany: db.prepare('SELECT m.name, m.data, m.etag, m.modified, m.cached_at, m.is_full FROM metadata m JOIN lookup_keys l ON m.name = l.name'),
    pendingWrites: [],
    pendingByName: new Map(),
    pendingCachedAtUpdates: new Map(),
    flushScheduled: false,
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

  getHeaders (name: string): MetadataHeaders | undefined {
    const pending = this.state.pendingByName.get(name)
    if (pending) {
      return {
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
      }
    }
    const row = this.state.stmtGet.get(name) as { etag: string | null, modified: string | null } | undefined
    if (!row) return undefined
    return {
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
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
    const row = this.state.stmtGet.get(name) as {
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
      cachedAt: this.state.pendingCachedAtUpdates.get(name) ?? row.cached_at ?? undefined,
      isFull: row.is_full === 1,
    }
  }

  queueSet (
    name: string,
    data: string,
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
    this.scheduleFlush()
  }

  updateCachedAt (name: string, cachedAt: number): void {
    const pending = this.state.pendingByName.get(name)
    if (pending) {
      pending.cachedAt = cachedAt
      return
    }
    this.state.pendingCachedAtUpdates.set(name, cachedAt)
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
    return result.changes > 0
  }

  listNames (): string[] {
    const rows = this.state.stmtListNames.all()
    return (rows as Array<{ name: string }>).map((r) => r.name)
  }

  getMany (names: string[]): Record<string, MetadataRow> {
    if (names.length === 0) return {}
    const result: Record<string, MetadataRow> = {}
    const remainingNames: string[] = []

    for (const name of names) {
      const pending = this.state.pendingByName.get(name)
      if (pending) {
        result[name] = {
          data: pending.data,
          etag: pending.etag ?? undefined,
          modified: pending.modified ?? undefined,
          cachedAt: pending.cachedAt,
          isFull: pending.isFull,
        }
      } else {
        remainingNames.push(name)
      }
    }

    if (remainingNames.length === 0) return result

    this.state.db.exec('DELETE FROM lookup_keys')
    const stmtInsertKey = this.state.db.prepare('INSERT INTO lookup_keys (name) VALUES (?)')
    this.state.db.exec('BEGIN TRANSACTION')
    try {
      for (const name of remainingNames) {
        stmtInsertKey.run(name)
      }
      this.state.db.exec('COMMIT')
    } catch (err) {
      this.state.db.exec('ROLLBACK')
      throw err
    }

    const rows = this.state.stmtGetMany.all() as Array<{
      name: string
      data: string
      etag: string | null
      modified: string | null
      cached_at: number | null
      is_full: number
    }>

    for (const row of rows) {
      result[row.name] = {
        data: row.data,
        etag: row.etag ?? undefined,
        modified: row.modified ?? undefined,
        cachedAt: this.state.pendingCachedAtUpdates.get(row.name) ?? row.cached_at ?? undefined,
        isFull: row.is_full === 1,
      }
    }

    return result
  }

  close (): void {
    this.flush()
  }
}
