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

interface PendingIndexWrite {
  kind: 'index'
  name: string
  etag: string | null
  modified: string | null
  cachedAt: number
  distTags: string
  versions: string
  time: string | null
}

interface PendingManifestWrite {
  kind: 'manifest'
  name: string
  version: string
  type: string
  manifest: string
}

type PendingWrite = PendingIndexWrite | PendingManifestWrite

export class MetadataCache {
  private db: DatabaseSyncType
  private closed = false
  private pendingWrites: PendingWrite[] = []
  private flushScheduled = false
  private stmtGetIndex: StatementSync
  private stmtGetHeaders: StatementSync
  private stmtSetIndex: StatementSync
  private stmtGetManifest: StatementSync
  private stmtSetManifest: StatementSync
  private stmtDeleteIndex: StatementSync
  private stmtDeleteManifests: StatementSync
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
          dist_tags TEXT NOT NULL,
          versions TEXT NOT NULL,
          time TEXT
        ) WITHOUT ROWID
      `)
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS metadata_manifests (
          name TEXT NOT NULL,
          version TEXT NOT NULL,
          type TEXT NOT NULL,
          manifest TEXT NOT NULL,
          PRIMARY KEY (name, version, type)
        ) WITHOUT ROWID
      `)
    })
    this.stmtGetIndex = this.db.prepare(
      'SELECT dist_tags, versions, time, etag, modified, cached_at FROM metadata_index WHERE name = ?'
    )
    this.stmtGetHeaders = this.db.prepare(
      'SELECT etag, modified FROM metadata_index WHERE name = ?'
    )
    this.stmtSetIndex = this.db.prepare(
      'INSERT OR REPLACE INTO metadata_index (name, etag, modified, cached_at, dist_tags, versions, time) VALUES (?, ?, ?, ?, ?, ?, ?)'
    )
    this.stmtGetManifest = this.db.prepare(
      'SELECT manifest FROM metadata_manifests WHERE name = ? AND version = ? AND type IN (?, \'full\') ORDER BY CASE type WHEN ? THEN 0 ELSE 1 END LIMIT 1'
    )
    this.stmtSetManifest = this.db.prepare(
      'INSERT OR REPLACE INTO metadata_manifests (name, version, type, manifest) VALUES (?, ?, ?, ?)'
    )
    this.stmtDeleteIndex = this.db.prepare('DELETE FROM metadata_index WHERE name = ?')
    this.stmtDeleteManifests = this.db.prepare('DELETE FROM metadata_manifests WHERE name = ?')
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

  private findPendingIndex (name: string): PendingIndexWrite | undefined {
    for (let i = this.pendingWrites.length - 1; i >= 0; i--) {
      const w = this.pendingWrites[i]
      if (w.kind === 'index' && w.name === name) return w
    }
    return undefined
  }

  private findPendingManifest (name: string, version: string, type: string): PendingManifestWrite | undefined {
    let fullFallback: PendingManifestWrite | undefined
    for (let i = this.pendingWrites.length - 1; i >= 0; i--) {
      const w = this.pendingWrites[i]
      if (w.kind !== 'manifest' || w.name !== name || w.version !== version) continue
      if (w.type === type) return w
      if (w.type === 'full' && !fullFallback) fullFallback = w
    }
    return fullFallback
  }

  /**
   * Get the conditional-request headers for a package.
   * Cheap — no manifest data touched.
   */
  getHeaders (name: string): MetadataHeaders | undefined {
    const pending = this.findPendingIndex(name)
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

  /**
   * Get the index data for resolution: dist-tags, version keys, time, and cache headers.
   * Does NOT load per-version manifests — very cheap.
   */
  getIndex (name: string): MetadataIndex | null {
    const pending = this.findPendingIndex(name)
    if (pending) {
      return {
        distTags: pending.distTags,
        versions: pending.versions,
        time: pending.time ?? undefined,
        etag: pending.etag ?? undefined,
        modified: pending.modified ?? undefined,
        cachedAt: pending.cachedAt,
      }
    }
    const row = sqliteRetry(() => this.stmtGetIndex.get(name)) as {
      dist_tags: string
      versions: string
      time: string | null
      etag: string | null
      modified: string | null
      cached_at: number | null
    } | undefined
    if (!row) return null
    return {
      distTags: row.dist_tags,
      versions: row.versions,
      time: row.time ?? undefined,
      etag: row.etag ?? undefined,
      modified: row.modified ?? undefined,
      cachedAt: row.cached_at ?? undefined,
    }
  }

  /**
   * Get a single version's manifest. Falls back from requested type to 'full'.
   */
  getManifest (name: string, version: string, type: string): string | null {
    const pending = this.findPendingManifest(name, version, type)
    if (pending) return pending.manifest
    const row = sqliteRetry(() => this.stmtGetManifest.get(name, version, type, type)) as { manifest: string } | undefined
    return row?.manifest ?? null
  }

  /**
   * Queue index + manifests from a parsed PackageMeta.
   */
  queueWrite (
    name: string,
    type: string,
    meta: {
      'dist-tags'?: Record<string, string>
      versions?: Record<string, { version?: string, deprecated?: string }> | null
      time?: Record<string, string> & { unpublished?: unknown }
      modified?: string
    },
    opts: { etag?: string, cachedAt: number }
  ): void {
    if (!meta['dist-tags'] || !meta.versions) return
    // Build compact versions map (version → {deprecated?} only)
    const versionsCompact: Record<string, { deprecated?: string }> = {}
    for (const [v, manifest] of Object.entries(meta.versions)) {
      versionsCompact[v] = manifest.deprecated ? { deprecated: manifest.deprecated as string } : {}
    }

    this.pendingWrites.push({
      kind: 'index',
      name,
      etag: opts.etag ?? null,
      modified: meta.modified ?? meta.time?.modified ?? null,
      cachedAt: opts.cachedAt,
      distTags: JSON.stringify(meta['dist-tags']),
      versions: JSON.stringify(versionsCompact),
      time: meta.time ? JSON.stringify(meta.time) : null,
    })

    for (const [v, manifest] of Object.entries(meta.versions)) {
      this.pendingWrites.push({
        kind: 'manifest',
        name,
        version: v,
        type,
        manifest: JSON.stringify(manifest),
      })
    }

    if (!this.flushScheduled) {
      this.flushScheduled = true
      process.nextTick(() => this.flush())
    }
  }

  /**
   * Update cachedAt without rewriting data.
   */
  updateCachedAt (name: string, cachedAt: number): void {
    sqliteRetry(() => {
      this.stmtUpdateCachedAt.run(cachedAt, name)
    })
  }

  /**
   * Flush all pending writes in a single transaction.
   */
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
          if (w.kind === 'index') {
            this.stmtSetIndex.run(w.name, w.etag, w.modified, w.cachedAt, w.distTags, w.versions, w.time)
          } else {
            this.stmtSetManifest.run(w.name, w.version, w.type, w.manifest)
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

  /**
   * Delete all data for a package.
   */
  delete (name: string): boolean {
    let changes!: number | bigint
    sqliteRetry(() => {
      this.db.exec('BEGIN IMMEDIATE')
      let committed = false
      try {
        const r1 = this.stmtDeleteIndex.run(name)
        this.stmtDeleteManifests.run(name)
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

  /**
   * List all package names.
   */
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
