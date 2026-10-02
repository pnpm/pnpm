import assert from 'node:assert/strict'
import { once } from 'node:events'
import { test } from 'node:test'
import { Worker } from 'node:worker_threads'

import { createGuestOperations } from './guest-operations.mjs'

test('guest imports transfer an asynchronous response through a worker without polling', async () => {
  const services = createGuestOperations({
    async dispatch (request) {
      assert.deepEqual(request, { operation: 'echo', text: 'héllo' })
      await new Promise(resolve => setTimeout(resolve, 10))
      return { bytes: Uint8Array.of(0, 128, 255) }
    },
    async close () {},
  })
  const importsUrl = new URL('./guest-imports.mjs', import.meta.url).href
  const source = `
    const { parentPort } = require('node:worker_threads');
    (async () => {
      const { createGuestImports } = await import(${JSON.stringify(importsUrl)});
      const memory = new WebAssembly.Memory({ initial: 1 });
      const imports = createGuestImports(memory, message => parentPort.postMessage(message));
      const request = new TextEncoder().encode(JSON.stringify({ operation: 'echo', text: 'héllo' }));
      new Uint8Array(memory.buffer).set(request);
      const id = imports.operation_start(0, request.length);
      const completed = imports.wait_completion();
      const length = imports.response_len(id);
      const copied = imports.response_read(id, 0, length);
      const response = JSON.parse(new TextDecoder().decode(new Uint8Array(memory.buffer, 0, copied)));
      parentPort.postMessage({ result: { id, completed, length, copied, response } });
    })().catch(error => { throw error });
  `
  const worker = new Worker(source, { eval: true })
  services.attach(worker)
  try {
    const result = await new Promise((resolve, reject) => {
      worker.on('error', reject)
      worker.on('message', message => { if (message.result) resolve(message.result) })
    })
    assert.equal(result.id, result.completed)
    assert.equal(result.length, result.copied)
    assert.deepEqual(result.response, { ok: true, value: { bytes: [0, 128, 255] } })
  } finally {
    await worker.terminate()
    await services.close()
  }
})

test('cancellation aborts a pending operation and shutdown wakes a waiting completion pump', async () => {
  let signal
  const services = createGuestOperations({
    async dispatch (_request, options) {
      signal = options.signal
      await once(signal, 'abort')
      throw new Error('aborted')
    },
    async close () {},
  })
  const id = await services.dispatch('start', [{ operation: 'pending' }])
  await services.dispatch('cancel', [id])
  assert.equal(signal.aborted, true)
  assert.equal(await services.dispatch('length', [id]), -2)
  const waiting = services.dispatch('wait', [])
  await services.close()
  assert.equal(await waiting, 0)
})

test('errors cross the guest boundary with their code and message', async () => {
  const services = createGuestOperations({
    async dispatch () { throw Object.assign(new Error('missing program'), { code: 'ENOENT' }) },
    async close () {},
  })
  try {
    const id = await services.dispatch('start', [{ operation: 'failure' }])
    assert.equal(await services.dispatch('wait', []), id)
    const length = await services.dispatch('length', [id])
    const bytes = await services.dispatch('read', [id, length])
    assert.deepEqual(JSON.parse(new TextDecoder().decode(bytes)), {
      ok: false, error: { message: 'missing program', code: 'ENOENT' },
    })
    assert.equal(await services.dispatch('length', [id]), -2)
  } finally {
    await services.close()
  }
})

test('a resource created after cancellation is closed before shutdown', async () => {
  let resolveRequest
  const closed = []
  const services = createGuestOperations({
    async dispatch (request) {
      if (request.operation === 'resource.close') {
        closed.push(request.handle)
        return
      }
      return new Promise(resolve => { resolveRequest = resolve })
    },
    async close () {},
  })
  const id = await services.dispatch('start', [{ operation: 'network.request' }])
  await services.dispatch('cancel', [id])
  resolveRequest({ handle: 7 })
  await new Promise(resolve => setImmediate(resolve))
  assert.deepEqual(closed, [7])
  await services.close()
})

test('shutdown kills a child before waiting for its pending process.wait request', { timeout: 5000 }, async () => {
  const services = createGuestOperations()
  const id = await services.dispatch('start', [{
    operation: 'process.spawn', program: process.execPath, args: ['-e', 'setInterval(() => {}, 1000)'],
  }])
  assert.equal(await services.dispatch('wait', []), id)
  const length = await services.dispatch('length', [id])
  const response = JSON.parse(new TextDecoder().decode(await services.dispatch('read', [id, length])))
  assert.equal(response.ok, true)
  await services.dispatch('start', [{ operation: 'process.wait', handle: response.value.handle }])
  await services.close()
})

for (const [name, metadata] of [
  ['oversized', 'x'.repeat(1024 * 1024)],
  ['unserializable', 1n],
]) {
  test(`${name} responses close their resource and deliver an error`, async () => {
    const closed = []
    const services = createGuestOperations({
      async dispatch (request) {
        if (request.operation === 'resource.close') closed.push(request.handle)
        else return { handle: 7, metadata }
      },
      async close () {},
    })
    try {
      const id = await services.dispatch('start', [{ operation: 'network.request' }])
      assert.equal(await services.dispatch('wait', []), id)
      const length = await services.dispatch('length', [id])
      const bytes = await services.dispatch('read', [id, length])
      const response = JSON.parse(new TextDecoder().decode(bytes))
      assert.equal(response.ok, false)
      assert.equal(typeof response.error.message, 'string')
      assert.deepEqual(closed, [7])
      await services.dispatch('cancel', [id])
      assert.deepEqual(closed, [7])
    } finally {
      await services.close()
    }
  })
}

test('failed cleanup of an oversized response still wakes the guest and surfaces at shutdown', async () => {
  const cleanupError = new Error('stream cancellation failed')
  const services = createGuestOperations({
    async dispatch (request) {
      if (request.operation === 'resource.close') throw cleanupError
      return { handle: 7, metadata: 'x'.repeat(1024 * 1024) }
    },
    async close () {},
  })
  const id = await services.dispatch('start', [{ operation: 'network.request' }])
  assert.equal(await services.dispatch('wait', []), id)
  const length = await services.dispatch('length', [id])
  const bytes = await services.dispatch('read', [id, length])
  assert.equal(JSON.parse(new TextDecoder().decode(bytes)).ok, false)
  await assert.rejects(services.close(), error => error instanceof AggregateError && error.errors.includes(cleanupError))
})

test('fetch timeout causes cross the guest boundary without exposing cause messages', async () => {
  for (const code of ['ETIMEDOUT', 'ESOCKETTIMEDOUT', 'UND_ERR_CONNECT_TIMEOUT']) {
    const services = createGuestOperations({
      async dispatch () {
        const cause = Object.assign(new Error('connection to https://user:secret@example.test failed'), { code })
        throw new TypeError('fetch failed', { cause })
      },
      async close () {},
    })
    try {
      const id = await services.dispatch('start', [{ operation: 'network.request' }])
      assert.equal(await services.dispatch('wait', []), id)
      const length = await services.dispatch('length', [id])
      const bytes = await services.dispatch('read', [id, length])
      assert.deepEqual(JSON.parse(new TextDecoder().decode(bytes)), {
        ok: false, error: { message: 'fetch failed', code },
      })
    } finally {
      await services.close()
    }
  }
})
