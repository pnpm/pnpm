import fs from 'node:fs'
import { createRequire } from 'node:module'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'

import { Packr } from 'msgpackr'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

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
 * Create a cache key from registry URL and package name.
 * The key is `${registry}\t${pkgName}` — tab-separated.
 * Tab characters are not valid in registry URLs or package names, so this is unambiguous.
 */
export function registryMetadataCacheKey (registry: string, pkgName: string): string {
  return `${registry}\t${pkgName}`
}

const openInstances = new Set<RegistryMetadataCache>()

/**
 * Close all open RegistryMetadataCache instances.
 * Useful in tests that need to remove the cache directory.
 */
export function closeAllRegistryMetadataCaches (): void {
  for (const cache of openInstances) {
    cache.close()
  }
}

export interface RegistryMetadataHeaders {
  etag?: string
  modified?: string
}

export class RegistryMetadataCache {
  private db: DatabaseSyncType
  private closed = false
  private stmtGet: StatementSync
  private stmtSet: StatementSync
  private stmtHas: StatementSync
  private stmtGetHeaders: StatementSync
  private readonly exitHandler: () => void

  constructor (cacheDir: string) {
    const dbPath = `${cacheDir}/registry-metadata.db`
    fs.mkdirSync(cacheDir, { recursive: true })
    this.db = new DatabaseSync(dbPath)
    // Set busy_timeout FIRST so SQLite's internal busy handler is active
    // during all subsequent operations. On Windows, file locking is mandatory
    // and concurrent processes (e.g. parallel pnpm calls) will contend.
    this.db.exec('PRAGMA busy_timeout=5000')
    sqliteRetry(() => {
      this.db.exec('PRAGMA journal_mode=WAL')
      this.db.exec('PRAGMA synchronous=NORMAL')
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS registry_metadata (
          key TEXT PRIMARY KEY,
          data BLOB NOT NULL,
          etag TEXT,
          modified TEXT
        ) WITHOUT ROWID
      `)
    })
    this.stmtGet = this.db.prepare('SELECT data FROM registry_metadata WHERE key = ?')
    this.stmtSet = this.db.prepare('INSERT OR REPLACE INTO registry_metadata (key, data, etag, modified) VALUES (?, ?, ?, ?)')
    this.stmtHas = this.db.prepare('SELECT 1 FROM registry_metadata WHERE key = ?')
    this.stmtGetHeaders = this.db.prepare('SELECT etag, modified FROM registry_metadata WHERE key = ?')
    this.exitHandler = () => this.close()
    // Multiple RegistryMetadataCache instances may be created (e.g. in tests), each adding
    // an exit listener. Raise the limit to avoid MaxListenersExceededWarning.
    // Skip when maxListeners is 0 (unlimited).
    const currentMax = process.getMaxListeners()
    if (currentMax !== 0 && currentMax < openInstances.size + 11) {
      process.setMaxListeners(Math.max(currentMax + 10, openInstances.size + 11))
    }
    process.on('exit', this.exitHandler)
    openInstances.add(this)
  }

  get (pkgName: string, registry: string): PackageMeta | undefined {
    const key = registryMetadataCacheKey(registry, pkgName)
    const row = sqliteRetry(() => this.stmtGet.get(key)) as { data: Uint8Array } | undefined
    if (row) {
      return packr.unpack(row.data) as PackageMeta
    }
    return undefined
  }

  set (pkgName: string, registry: string, meta: PackageMeta): void {
    const key = registryMetadataCacheKey(registry, pkgName)
    const buffer = packr.pack(meta)
    sqliteRetry(() => {
      this.stmtSet.run(key, buffer, meta.etag ?? null, meta.modified ?? null)
    })
  }

  has (pkgName: string, registry: string): boolean {
    const key = registryMetadataCacheKey(registry, pkgName)
    return sqliteRetry(() => this.stmtHas.get(key)) != null
  }

  getHeaders (pkgName: string, registry: string): RegistryMetadataHeaders | undefined {
    const key = registryMetadataCacheKey(registry, pkgName)
    const row = sqliteRetry(() => this.stmtGetHeaders.get(key)) as { etag: string | null, modified: string | null } | undefined
    if (row) {
      const headers: RegistryMetadataHeaders = {}
      if (row.etag != null) {
        headers.etag = row.etag
      }
      if (row.modified != null) {
        headers.modified = row.modified
      }
      return Object.keys(headers).length > 0 ? headers : undefined
    }
    return undefined
  }

  close (): void {
    if (this.closed) return
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