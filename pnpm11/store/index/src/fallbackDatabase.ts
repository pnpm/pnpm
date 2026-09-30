import { randomInt } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import type { DatabaseSync as DatabaseSyncType, StatementSync } from 'node:sqlite'
import { threadId } from 'node:worker_threads'

import { PnpmError } from '@pnpm/error'
import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
import { Packr } from 'msgpackr'

const fallbackPackr = new Packr({
  useRecords: false,
  moreTypes: true,
})

const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))
const fallbackDatabases = new WeakSet<object>()

const FALLBACK_INDEX_FILE = 'index.fallback'

/**
 * Returns a store index connection that has `exec`, for hosts such as
 * StackBlitz WebContainers whose `DatabaseSync` lacks it.
 *
 * `db` is returned unchanged when it has `exec`, and gains an `exec` that runs
 * through `prepare` when it lacks only that. When `db` cannot be adapted, it is
 * closed and a connection backed by `index.fallback` in `storeDir` is returned.
 */
export function adaptStoreDatabase (db: SqliteConnection, storeDir: string): DatabaseSyncType {
  if (typeof db.prepare !== 'function') {
    closeSqliteQuietly(db)
    return createFallbackDatabase(storeDir)
  }
  if (typeof db.exec !== 'function') {
    const prepare = (db.prepare as (sql: string) => { run?: (() => unknown) | undefined }).bind(db)
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
  const state: FallbackState = {
    dataPath: path.join(storeDir, FALLBACK_INDEX_FILE),
    rows: new Map(),
    revision: -1,
    tx: null,
  }

  const db = {
    exec (sql: string): void {
      prepareFallbackStatement(state, sql).run()
    },
    prepare (sql: string): StatementSync {
      return prepareFallbackStatement(state, sql) as unknown as StatementSync
    },
    close (): void {
      state.tx = null
    },
  }
  fallbackDatabases.add(db)
  return db as unknown as DatabaseSyncType
}

interface FallbackState {
  dataPath: string
  rows: Map<string, Uint8Array>
  revision: number
  tx: Map<string, Uint8Array> | null
}

interface Snapshot {
  generation: number
  rows: Map<string, Uint8Array>
}

interface FallbackStatementDefinition {
  matches: (normalizedSql: string) => boolean
  createHandlers: (state: FallbackState) => Partial<BoundStatement>
}

const NO_OP_SQL_PREFIXES = ['pragma ', 'vacuum', 'create table']

const FALLBACK_STATEMENTS: FallbackStatementDefinition[] = [
  {
    matches: (normalizedSql) => NO_OP_SQL_PREFIXES.some((prefix) => normalizedSql.startsWith(prefix)),
    createHandlers: () => ({}),
  },
  {
    matches: sqlStartsWith('begin'),
    createHandlers: (state) => ({ run: () => runAndReportNoChanges(() => begin(state)) }),
  },
  {
    matches: sqlEquals('commit'),
    createHandlers: (state) => ({ run: () => runAndReportNoChanges(() => commit(state)) }),
  },
  {
    matches: sqlEquals('rollback'),
    createHandlers: (state) => ({ run: () => runAndReportNoChanges(() => rollback(state)) }),
  },
  {
    matches: sqlStartsWith('select data from package_index where key'),
    createHandlers: (state) => ({
      get: (key: unknown) => {
        const data = view(state).get(String(key))
        return data == null ? undefined : { data }
      },
    }),
  },
  {
    matches: sqlStartsWith('select 1 from package_index where key'),
    createHandlers: (state) => ({
      get: (key: unknown) => view(state).has(String(key)) ? { 1: 1 } : undefined,
    }),
  },
  {
    matches: sqlStartsWith('select key, data from package_index'),
    createHandlers: (state) => ({
      iterate: () => iterateEntries(view(state)),
    }),
  },
  {
    matches: sqlStartsWith('select key from package_index'),
    createHandlers: (state) => ({
      iterate: () => iterateKeys(view(state)),
    }),
  },
  {
    matches: sqlStartsWith('insert or replace into package_index'),
    createHandlers: (state) => ({
      run: (key: unknown, data: unknown) => {
        const bytes = copyBytes(data)
        mutate(state, map => {
          map.set(String(key), bytes)
        })
        return done(1)
      },
    }),
  },
  {
    matches: sqlStartsWith('delete from package_index where key'),
    createHandlers: (state) => ({
      run: (key: unknown) => {
        let changes = 0
        mutate(state, map => {
          changes = map.delete(String(key)) ? 1 : 0
        })
        return done(changes)
      },
    }),
  },
]

function prepareFallbackStatement (state: FallbackState, sql: string): BoundStatement {
  const normalized = normalizeSql(sql)
  const definition = FALLBACK_STATEMENTS.find(({ matches }) => matches(normalized))
  if (definition == null) {
    throw new Error(`The store index fallback cannot run this SQL: ${sql}`)
  }
  return statement(definition.createHandlers(state))
}

function sqlStartsWith (prefix: string): (normalizedSql: string) => boolean {
  return (normalizedSql) => normalizedSql.startsWith(prefix)
}

function sqlEquals (expected: string): (normalizedSql: string) => boolean {
  return (normalizedSql) => normalizedSql === expected
}

function runAndReportNoChanges (action: () => void): RunResult {
  action()
  return done(0)
}

function begin (state: FallbackState): void {
  if (state.tx != null) {
    throw new Error('Cannot begin a store index fallback transaction while one is open')
  }
  reloadIfStale(state)
  state.tx = new Map(state.rows)
}

function commit (state: FallbackState): void {
  if (state.tx == null) {
    throw new Error('Cannot commit a store index fallback transaction when none is open')
  }
  const committed = state.tx
  const previous = state.rows
  state.tx = null
  state.rows = committed
  try {
    writeThrough(state)
  } catch (err: unknown) {
    state.rows = previous
    state.revision = -1
    reloadIfStale(state)
    throw err
  }
}

function rollback (state: FallbackState): void {
  state.tx = null
}

function view (state: FallbackState): Map<string, Uint8Array> {
  if (state.tx != null) return state.tx
  reloadIfStale(state)
  return state.rows
}

function mutate (state: FallbackState, apply: (map: Map<string, Uint8Array>) => void): void {
  if (state.tx != null) {
    apply(state.tx)
    return
  }
  reloadIfStale(state)
  const previous = state.rows
  const nextRows = new Map(state.rows)
  apply(nextRows)
  state.rows = nextRows
  try {
    writeThrough(state)
  } catch (err: unknown) {
    state.rows = previous
    throw err
  }
}

function reloadIfStale (state: FallbackState): void {
  if (state.tx != null) return
  const snapshot = readSnapshot(state)
  if (snapshot.generation === state.revision) return
  state.rows = snapshot.rows
  state.revision = snapshot.generation
}

function readSnapshot (state: FallbackState): Snapshot {
  for (let attempt = 0; attempt < 20; attempt++) {
    const snapshot = tryReadSnapshot(state)
    if (snapshot != null) return snapshot
    sleepSync(1)
  }
  throw new PnpmError(
    'STORE_INDEX_FALLBACK_CORRUPT',
    `Could not read the store index fallback file at ${state.dataPath}`
  )
}

/**
 * Returns undefined when the file is mid-write (torn header, a header that
 * does not match the body, or an undecodable body), so the caller retries.
 */
function tryReadSnapshot (state: FallbackState): Snapshot | undefined {
  const generation = readGeneration(state.dataPath)
  if (generation === undefined) return undefined
  if (generation === state.revision) {
    return { generation, rows: state.rows }
  }
  if (generation === 0) {
    return { generation: 0, rows: new Map() }
  }
  const packed = readFallbackFile(state.dataPath)
  if (packed == null) {
    return { generation: 0, rows: new Map() }
  }
  if (packed.length < 4 || packed.readUInt32BE(0) !== generation) return undefined
  const decodedRows = decodeRows(packed.subarray(4))
  if (decodedRows == null) return undefined
  return { generation, rows: decodedRows }
}

function readFallbackFile (dataPath: string): Buffer | undefined {
  try {
    return fs.readFileSync(dataPath)
  } catch (err: unknown) {
    if (!isEnoent(err)) throw err
    return undefined
  }
}

function writeThrough (state: FallbackState): void {
  const next = nextGeneration(state.revision)
  const body = fallbackPackr.pack([...state.rows.entries()])
  const payload = new Uint8Array(4 + body.length)
  new DataView(payload.buffer, payload.byteOffset, payload.byteLength).setUint32(0, next)
  payload.set(body, 4)
  const tmp = `${state.dataPath}.${process.pid}.${threadId}.tmp`
  try {
    fs.writeFileSync(tmp, payload)
    renameFileWithRetry(tmp, state.dataPath)
  } catch (err: unknown) {
    try {
      fs.rmSync(tmp, { force: true })
    } catch {}
    throw err
  }
  state.revision = next
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

// Generations are random rather than counted. Two processes that write from
// the same snapshot would otherwise stamp the same number, and each would keep
// trusting its own rows and overwrite the other's entries on its next write.
// Zero is reserved for a missing file.
function nextGeneration (current: number): number {
  let next: number
  do {
    next = randomInt(1, 2 ** 32)
  } while (next === current)
  return next
}

// A short header lets readers notice a new snapshot without decoding it.
// Returns 0 for a missing file and undefined for a header no writer stamps:
// a torn file, or a zero that {@link nextGeneration} never produces.
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
    return buf.readUInt32BE(0) || undefined
  } finally {
    fs.closeSync(fd)
  }
}

function isEnoent (err: unknown): boolean {
  return typeof err === 'object' && err != null && 'code' in err && err.code === 'ENOENT'
}

function sleepSync (ms: number): void {
  Atomics.wait(sleepBuffer, 0, 0, ms)
}
