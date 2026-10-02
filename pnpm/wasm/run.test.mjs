import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import http from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { runWasm } from './run.mjs'

const artifact = fileURLToPath(new URL('../../target/wasm-runtime-probe/wasm32-wasip1-threads/debug/pnpm-wasm-runtime-probe.wasm', import.meta.url))

test('threaded Rust runtime shares files and uses async HTTP and process services', { timeout: 30000 }, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'pnpm-wasm-runtime-'))
  const registry = http.createServer((_request, response) => {
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ name: 'is-odd' }))
  })
  await new Promise(resolve => registry.listen(0, '127.0.0.1', resolve))
  try {
    assert.equal(await runWasm(artifact, {
      env: {
        ...process.env,
        PROBE_DIR: directory,
        PROBE_REGISTRY_URL: `http://127.0.0.1:${registry.address().port}/is-odd/latest`,
      },
    }), 0)
  } finally {
    registry.closeAllConnections()
    await new Promise(resolve => registry.close(resolve))
    await rm(directory, { recursive: true, force: true })
  }
})

test('a guest panic rejects the run and releases its workers', { timeout: 10000 }, async () => {
  await assert.rejects(runWasm(artifact, { env: {} }), { name: 'RuntimeError' })
})

test('process exit from a guest thread terminates the entire run', { timeout: 10000 }, async () => {
  assert.equal(await runWasm(artifact, { env: { PROBE_THREAD_EXIT: '1' } }), 23)
})


test('invalid worker pool sizes reject before allocating runtime resources', async () => {
  await Promise.all([0, -1, 1.5, Number.NaN, Infinity].map(workers =>
    assert.rejects(runWasm('/unused.wasm', { workers }), { name: 'RangeError' }),
  ))
})
