import { createRequire } from 'node:module'
import type { DatabaseSync as _DatabaseSync } from 'node:sqlite'

// Jest's ESM module resolver doesn't handle node:sqlite.
// Use createRequire to load it at runtime.
const { DatabaseSync } = createRequire(import.meta.url)('node:sqlite') as typeof import('node:sqlite')

/**
 * SQLite-backed file store for fast batch reads.
 * Files are stored as blobs keyed by hex digest.
 * Much faster than 33K individual readFileSync calls.
 */
export class FileStore {
  private db: _DatabaseSync
  private getStmt!: ReturnType<_DatabaseSync['prepare']>
  private hasStmt!: ReturnType<_DatabaseSync['prepare']>
  private insertStmt!: ReturnType<_DatabaseSync['prepare']>

  constructor (dbPath: string) {
    this.db = new DatabaseSync(dbPath)
    this.db.exec('PRAGMA busy_timeout=5000')
    this.db.exec('PRAGMA journal_mode=WAL')
    this.db.exec('PRAGMA synchronous=NORMAL')
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS files (
        digest TEXT PRIMARY KEY,
        content BLOB NOT NULL,
        size INTEGER NOT NULL,
        executable INTEGER NOT NULL DEFAULT 0
      )
    `)
    // /v1/files is a hot path (thousands of .get() calls per request), so
    // prepare statements once in the constructor and reuse them.
    this.getStmt = this.db.prepare('SELECT content, size FROM files WHERE digest = ?')
    this.hasStmt = this.db.prepare('SELECT 1 FROM files WHERE digest = ?')
    this.insertStmt = this.db.prepare(
      'INSERT OR IGNORE INTO files (digest, content, size, executable) VALUES (?, ?, ?, ?)'
    )
  }

  has (digest: string): boolean {
    return this.hasStmt.get(digest) !== undefined
  }

  /**
   * Bulk insert pre-read file contents. Runs in a transaction for speed.
   * Callers pass the already-read buffer so we don't re-read from disk.
   */
  importMany (files: Array<{ digest: string, content: Buffer, executable: boolean }>): number {
    let imported = 0
    this.db.exec('BEGIN')
    try {
      for (const file of files) {
        const result = this.insertStmt.run(file.digest, file.content, file.content.length, file.executable ? 1 : 0)
        if (result.changes > 0) imported++
      }
      this.db.exec('COMMIT')
    } catch (err) {
      this.db.exec('ROLLBACK')
      throw err
    }
    return imported
  }

  /**
   * Get file content and size for building the archive response.
   */
  get (digest: string): { content: Buffer, size: number } | undefined {
    return this.getStmt.get(digest) as { content: Buffer, size: number } | undefined
  }

  close (): void {
    this.db.close()
  }
}
