import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { once } from 'node:events'
import fs from 'node:fs'
import { DatabaseSync } from 'node:sqlite'
import { WASI } from 'node:wasi'
import { Worker } from 'node:worker_threads'

console.log('WebContainer host:', process.version, process.platform, process.arch)
await runWasi()
await checkNodeHost()
console.log('pnpm-wasm-host-probe-ok')

async function runWasi () {
  const wasi = new WASI({
    version: 'preview1',
    args: ['pnpm-wasm-probe'],
    env: { PROBE_DIR: '/work' },
    preopens: { '/work': process.cwd() },
    returnOnExit: true,
  })
  const module = await WebAssembly.compile(fs.readFileSync(new URL('probe.wasm', import.meta.url)))
  console.log('WASM imports:', JSON.stringify(WebAssembly.Module.imports(module)))
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport })
  assert.equal(wasi.start(instance), 0)
  assert.equal(fs.readFileSync('output', 'utf8'), 'pnpm-wasm-probe')
  assert.equal(fs.readFileSync('linked', 'utf8'), 'pnpm-wasm-probe')
}

async function checkNodeHost () {
  fs.symlinkSync('output', 'symlink')
  assert.equal(fs.readFileSync('symlink', 'utf8'), 'pnpm-wasm-probe')
  assert.equal(randomBytes(32).length, 32)
  await checkWorker()
  const child = spawn(process.execPath, ['-e', 'process.exit(17)'], { stdio: 'inherit' })
  const [exitCode, signal] = await once(child, 'exit')
  assert.equal(exitCode, 17)
  assert.equal(signal, null)
  checkSqlite()
  const response = await fetch('https://registry.npmjs.org/is-odd/latest', { signal: AbortSignal.timeout(30_000) })
  assert.equal(response.status, 200)
  assert.ok(response.body)
  let bytes = 0
  for await (const chunk of response.body) bytes += chunk.length
  assert.ok(bytes > 0)
  console.log('Node host: symlinks, randomness, workers, subprocesses and HTTP streaming passed')
}

async function checkWorker () {
  const worker = new Worker("require('node:worker_threads').parentPort.postMessage(42)", { eval: true })
  try {
    assert.deepEqual(await once(worker, 'message'), [42])
  } finally {
    await worker.terminate()
  }
}

function checkSqlite () {
  const database = new DatabaseSync('probe.sqlite')
  const missing = ['exec', 'prepare', 'close'].filter(method => typeof database[method] !== 'function')
  if (missing.length > 0) {
    console.log('Node SQLite API unavailable; missing methods:', missing.join(', '))
    if (typeof database.close === 'function') database.close()
    return
  }
  try {
    database.exec('CREATE TABLE probe (value TEXT); INSERT INTO probe VALUES (\'persisted\')')
  } finally {
    database.close()
  }
  const reopened = new DatabaseSync('probe.sqlite')
  try {
    assert.equal(reopened.prepare('SELECT value FROM probe').get().value, 'persisted')
    console.log('Node SQLite persistence passed')
  } finally {
    reopened.close()
  }
}
