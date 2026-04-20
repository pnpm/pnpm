import { readdirSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { DatabaseSync as _DatabaseSync } from 'node:sqlite'

const { DatabaseSync } = createRequire(import.meta.url)('node:sqlite') as typeof import('node:sqlite')

// Matches @pnpm/resolving.npm-resolver's PackageMetaCache interface
// without adding a dependency on it.
interface PackageMetaCache {
  get: (key: string) => unknown | undefined
  set: (key: string, meta: unknown) => void
  has: (key: string) => boolean
}

/**
 * SQLite-backed PackageMetaCache for the pnpm agent server.
 *
 * Stores package metadata as pre-serialized JSON blobs keyed by cache key.
 * Much faster than reading hundreds of .jsonl files from disk on every
 * resolution — one indexed SQLite lookup vs file open + JSON parse.
 *
 * The server populates this at startup from the existing .jsonl cache files.
 * Subsequent metadata fetches (from npm) also update SQLite via `set()`.
 */
export class MetadataStore implements PackageMetaCache {
  private db: _DatabaseSync
  private getStmt!: ReturnType<_DatabaseSync['prepare']>
  private setStmt!: ReturnType<_DatabaseSync['prepare']>
  private hasStmt!: ReturnType<_DatabaseSync['prepare']>

  constructor (dbPath: string) {
    this.db = new DatabaseSync(dbPath)
    this.db.exec('PRAGMA busy_timeout=5000')
    this.db.exec('PRAGMA journal_mode=WAL')
    this.db.exec('PRAGMA synchronous=NORMAL')
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      )
    `)
    this.getStmt = this.db.prepare('SELECT value FROM meta WHERE key = ?')
    this.setStmt = this.db.prepare('INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)')
    this.hasStmt = this.db.prepare('SELECT 1 FROM meta WHERE key = ?')
  }

  get (key: string): unknown | undefined {
    const row = this.getStmt.get(key) as { value: string } | undefined
    if (!row) return undefined
    return JSON.parse(row.value)
  }

  set (key: string, meta: unknown): void {
    this.setStmt.run(key, JSON.stringify(meta))
  }

  has (key: string): boolean {
    return this.hasStmt.get(key) !== undefined
  }

  /**
   * Import all .jsonl metadata files from the cache directory into SQLite.
   * Each .jsonl file has: line 1 = headers (etag/modified), line 2 = metadata JSON.
   * We store the parsed metadata (with etag attached) under the cache key.
   */
  importFromCacheDir (cacheDir: string): number {
    const metaDirs = ['v11/metadata', 'v11/metadata-full', 'v11/metadata-full-filtered']
    let imported = 0

    this.db.exec('BEGIN')
    try {
      for (const metaDir of metaDirs) {
        const baseDir = path.join(cacheDir, metaDir)
        let registries: string[]
        try {
          registries = readdirSync(baseDir)
        } catch {
          continue
        }

        for (const registry of registries) {
          const regDir = path.join(baseDir, registry)
          let files: string[]
          try {
            files = readdirSync(regDir)
          } catch {
            continue
          }

          const isFullMeta = metaDir.includes('full')

          for (const file of files) {
            if (!file.endsWith('.jsonl')) continue
            const pkgName = decodeURIComponent(file.replace('.jsonl', ''))
            const cacheKey = isFullMeta ? `${pkgName}:full` : pkgName

            if (this.has(cacheKey)) continue

            try {
              const content = readFileSync(path.join(regDir, file), 'utf-8')
              const newlineIdx = content.indexOf('\n')
              if (newlineIdx === -1) continue

              const headerLine = content.substring(0, newlineIdx)
              const metaLine = content.substring(newlineIdx + 1)

              const headers = JSON.parse(headerLine)
              const meta = JSON.parse(metaLine)
              meta.etag = headers.etag
              meta.modified = headers.modified

              this.setStmt.run(cacheKey, JSON.stringify(meta))
              imported++
            } catch {
              // Skip corrupt files
            }
          }
        }
      }
      this.db.exec('COMMIT')
    } catch (err) {
      this.db.exec('ROLLBACK')
      throw err
    }

    return imported
  }

  close (): void {
    this.db.close()
  }
}
