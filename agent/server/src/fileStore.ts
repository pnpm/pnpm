import { readFileSync } from 'node:fs'
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
  }

  has (digest: string): boolean {
    const row = this.db.prepare('SELECT 1 FROM files WHERE digest = ?').get(digest) as { '1': number } | undefined
    return row !== undefined
  }

  /**
   * Import a file from the CAFS into the SQLite store.
   */
  importFromCafs (digest: string, cafsPath: string, executable: boolean): void {
    if (this.has(digest)) return
    const content = readFileSync(cafsPath)
    this.db.prepare(
      'INSERT OR IGNORE INTO files (digest, content, size, executable) VALUES (?, ?, ?, ?)'
    ).run(digest, content, content.length, executable ? 1 : 0)
  }

  /**
   * Bulk import files from CAFS. Runs in a transaction for speed.
   */
  importManyFromCafs (files: Array<{ digest: string, cafsPath: string, executable: boolean }>): number {
    const insert = this.db.prepare(
      'INSERT OR IGNORE INTO files (digest, content, size, executable) VALUES (?, ?, ?, ?)'
    )
    let imported = 0
    const existing = this.db.prepare('SELECT digest FROM files WHERE digest = ?')

    this.db.exec('BEGIN')
    try {
      for (const file of files) {
        const row = existing.get(file.digest) as { digest: string } | undefined
        if (row) continue
        const content = readFileSync(file.cafsPath)
        insert.run(file.digest, content, content.length, file.executable ? 1 : 0)
        imported++
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
    const row = this.db.prepare(
      'SELECT content, size FROM files WHERE digest = ?'
    ).get(digest) as { content: Buffer, size: number } | undefined
    return row
  }

  close (): void {
    this.db.close()
  }
}
