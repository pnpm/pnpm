import fs from 'node:fs'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { threadId } from 'node:worker_threads'

import { PnpmError } from '@pnpm/error'
import { Packr } from 'msgpackr'

const fallbackPackr = new Packr({
  useRecords: false,
  moreTypes: true,
})

const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))
const fallbackDatabases = new WeakSet<object>()

const FALLBACK_INDEX_FILE = 'index.fallback'

/**
 * Some hosts construct `DatabaseSync` without `exec`. Prepared statements run
 * the same SQL. When `prepare` is missing too, rows are stored in
 * `index.fallback`.
 */
export function adaptStoreDatabase (db: SqliteConnection, storeDir: string): DatabaseSyncType {
  if (typeof db.prepare !== 'function') {
    closeSqliteQuietly(db)
    return createFallbackDatabase(storeDir)
  }
  if (typeof db.exec !== 'function') {
    const prepare = db.prepare as (sql: string) => { run?: (() => unknown) | undefined }
    const exec = (sql: string): void => {
      const stmt = prepare(sql)
      if (typeof stmt?.run !== 'function') {
        throw new TypeError('DatabaseSync statement run is not a function')
      }
      stmt.run()
    }
    try {
      db.exec = exec
    } catch {
      closeSqliteQuietly(db)
      return createFallbackDatabase(storeDir)
    }
  }
  return db as DatabaseSyncType
}

export function createFallbackDatabase (storeDir: string): DatabaseSyncType {
  const dataPath = path.join(storeDir, FALLBACK_INDEX_FILE)
  let rows = new Map<string, Uint8Array>()
  let revision = -1
  let tx: Map<string, Uint8Array> | null = null

  const db = {
    exec (sql: string): void {
      prepare(sql).run()
    },
    prepare (sql: string): StatementSync {
      return prepare(sql) as unknown as StatementSync
    },
    close (): void {
      tx = null
    },
  }
  fallbackDatabases.add(db)
  return db as unknown as DatabaseSyncType

  function prepare (sql: string): BoundStatement {
    const normalized = normalizeSql(sql)
    if (
      normalized.startsWith('pragma ') ||
      normalized.startsWith('vacuum') ||
      normalized.startsWith('create table')
    ) {
      return statement({})
    }
    if (normalized.startsWith('begin')) {
      return statement({
        run: () => {
          begin()
          return done(0)
        },
      })
    }
    if (normalized === 'commit') {
      return statement({
        run: () => {
          commit()
          return done(0)
        },
      })
    }
    if (normalized === 'rollback') {
      return statement({
        run: () => {
          rollback()
          return done(0)
        },
      })
    }
    if (normalized.startsWith('select data from package_index where key')) {
      return statement({
        get: (key: unknown) => {
          const data = view().get(String(key))
          return data == null ? undefined : { data }
        },
      })
    }
    if (normalized.startsWith('select 1 from package_index where key')) {
      return statement({
        get: (key: unknown) => view().has(String(key)) ? { 1: 1 } : undefined,
      })
    }
    if (normalized.startsWith('select key, data from package_index')) {
      return statement({
        iterate: () => iterateEntries(view()),
      })
    }
    if (normalized.startsWith('select key from package_index')) {
      return statement({
        iterate: () => iterateKeys(view()),
      })
    }
    if (normalized.startsWith('insert or replace into package_index')) {
      return statement({
        run: (key: unknown, data: unknown) => {
          const bytes = copyBytes(data)
          mutate(map => {
            map.set(String(key), bytes)
          })
          return done(1)
        },
      })
    }
    if (normalized.startsWith('delete from package_index where key')) {
      return statement({
        run: (key: unknown) => {
          let changes = 0
          mutate(map => {
            changes = map.delete(String(key)) ? 1 : 0
          })
          return done(changes)
        },
      })
    }
    throw new Error(`The store index fallback cannot run this SQL: ${sql}`)
  }

  function begin (): void {
    if (tx != null) {
      throw new Error('Cannot begin a store index fallback transaction while one is open')
    }
    reloadIfStale()
    tx = new Map(rows)
  }

  function commit (): void {
    if (tx == null) {
      throw new Error('Cannot commit a store index fallback transaction when none is open')
    }
    const committed = tx
    const previous = rows
    tx = null
    rows = committed
    try {
      writeThrough()
    } catch (err: unknown) {
      rows = previous
      revision = -1
      reloadIfStale()
      throw err
    }
  }

  function rollback (): void {
    tx = null
  }

  function view (): Map<string, Uint8Array> {
    if (tx != null) return tx
    reloadIfStale()
    return rows
  }

  function mutate (apply: (map: Map<string, Uint8Array>) => void): void {
    if (tx != null) {
      apply(tx)
      return
    }
    reloadIfStale()
    const previous = rows
    const nextRows = new Map(rows)
    apply(nextRows)
    rows = nextRows
    try {
      writeThrough()
    } catch (err: unknown) {
      rows = previous
      throw err
    }
  }

  function reloadIfStale (): void {
    if (tx != null) return
    const snapshot = readSnapshot()
    if (snapshot.generation === revision) return
    rows = snapshot.rows
    revision = snapshot.generation
  }

  function readSnapshot (): { generation: number, rows: Map<string, Uint8Array> } {
    let sawMissing = false
    for (let attempt = 0; attempt < 20; attempt++) {
      const generation = readGeneration(dataPath)
      if (generation === undefined) {
        sleepSync(1)
        continue
      }
      if (generation === revision) {
        return { generation, rows }
      }
      if (generation === 0) {
        if (revision <= 0) return { generation: 0, rows: new Map() }
        sawMissing = true
        sleepSync(1)
        continue
      }
      let packed: Buffer
      try {
        packed = fs.readFileSync(dataPath)
      } catch (err: unknown) {
        if (!isEnoent(err)) throw err
        sawMissing = true
        sleepSync(1)
        continue
      }
      if (packed.length < 4 || packed.readUInt32BE(0) !== generation) {
        sleepSync(1)
        continue
      }
      const decodedRows = decodeRows(packed.subarray(4))
      if (decodedRows == null) {
        sleepSync(1)
        continue
      }
      return { generation, rows: decodedRows }
    }
    if (sawMissing) return { generation: 0, rows: new Map() }
    throw new PnpmError(
      'STORE_INDEX_FALLBACK_CORRUPT',
      `Could not read the store index fallback file at ${dataPath}`
    )
  }

  function writeThrough (): void {
    const next = revision + 1
    const body = fallbackPackr.pack([...rows.entries()])
    const payload = new Uint8Array(4 + body.length)
    new DataView(payload.buffer, payload.byteOffset, payload.byteLength).setUint32(0, next)
    payload.set(body, 4)
    const tmp = `${dataPath}.${process.pid}.${threadId}.tmp`
    try {
      fs.writeFileSync(tmp, payload)
      replaceFile(tmp, dataPath)
    } catch (err: unknown) {
      try {
        fs.rmSync(tmp, { force: true })
      } catch {}
      throw err
    }
    revision = next
  }
}

export function isFallbackDatabase (db: object): boolean {
  return fallbackDatabases.has(db)
}

export function isMissingSqliteMethod (err: unknown): boolean {
  return typeof err === 'object' &&
    err != null &&
    'message' in err &&
    typeof err.message === 'string' &&
    err.message.includes('is not a function')
}

export function closeSqliteQuietly (db: { close?: (() => void) | undefined }): void {
  try {
    db.close?.()
  } catch {}
}

interface SqliteConnection {
  exec?: unknown
  prepare?: unknown
  close?: (() => void) | undefined
}

interface RunResult {
  changes: number
  lastInsertRowid: number
}

interface BoundStatement {
  get: (...params: unknown[]) => unknown
  run: (...params: unknown[]) => RunResult
  iterate: () => IterableIterator<Record<string, unknown>>
}

function statement (handlers: Partial<BoundStatement>): BoundStatement {
  return {
    get: handlers.get ?? (() => undefined),
    run: handlers.run ?? (() => done(0)),
    iterate: handlers.iterate ?? function * () {},
  }
}

function done (changes: number): RunResult {
  return { changes, lastInsertRowid: 0 }
}

function normalizeSql (sql: string): string {
  return sql.replace(/\s+/g, ' ').trim().toLowerCase()
}

function copyBytes (data: unknown): Uint8Array {
  if (data instanceof Uint8Array) return new Uint8Array(data)
  throw new Error('Store index fallback expected a byte buffer')
}

function * iterateEntries (map: Map<string, Uint8Array>): IterableIterator<{ key: string, data: Uint8Array }> {
  for (const [key, data] of [...map.entries()]) {
    yield { key, data }
  }
}

function * iterateKeys (map: Map<string, Uint8Array>): IterableIterator<{ key: string }> {
  for (const key of [...map.keys()]) {
    yield { key }
  }
}

function decodeRows (body: Uint8Array): Map<string, Uint8Array> | undefined {
  let decoded: unknown
  try {
    decoded = fallbackPackr.unpack(body)
  } catch {
    return undefined
  }
  if (!Array.isArray(decoded)) return undefined
  const rows = new Map<string, Uint8Array>()
  for (const entry of decoded) {
    if (!Array.isArray(entry) || typeof entry[0] !== 'string' || !(entry[1] instanceof Uint8Array)) {
      return undefined
    }
    rows.set(entry[0], entry[1])
  }
  return rows
}

// A short header lets readers notice a new snapshot without decoding it.
// A torn or half-replaced file fails the length check and is retried.
function readGeneration (dataPath: string): number | undefined {
  let fd: number
  try {
    fd = fs.openSync(dataPath, 'r')
  } catch (err: unknown) {
    if (isEnoent(err)) return 0
    throw err
  }
  try {
    const buf = Buffer.alloc(4)
    const n = fs.readSync(fd, buf, 0, 4, 0)
    if (n < 4) return undefined
    return buf.readUInt32BE(0)
  } finally {
    fs.closeSync(fd)
  }
}

function replaceFile (from: string, to: string): void {
  try {
    fs.renameSync(from, to)
  } catch (err: unknown) {
    // Windows throws when the destination already exists.
    if (process.platform !== 'win32' || !isRenameConflict(err)) throw err
    fs.rmSync(to, { force: true })
    fs.renameSync(from, to)
  }
}

function isRenameConflict (err: unknown): boolean {
  return isCode(err, 'EEXIST') || isCode(err, 'EPERM') || isCode(err, 'ENOTEMPTY')
}

function isEnoent (err: unknown): boolean {
  return isCode(err, 'ENOENT')
}

function isCode (err: unknown, code: string): boolean {
  return typeof err === 'object' && err != null && 'code' in err && err.code === code
}

function sleepSync (ms: number): void {
  Atomics.wait(sleepBuffer, 0, 0, ms)
}
