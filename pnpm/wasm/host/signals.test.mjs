import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'

import { createSignals } from './signals.mjs'

test('signal relay retains signals between requests and removes listeners on close', async () => {
  const source = new EventEmitter()
  const signals = createSignals(source)
  assert.equal(source.listenerCount('SIGINT'), 0)
  const first = signals.next()
  source.emit('SIGINT')
  source.emit('SIGTERM')
  assert.deepEqual(await first, { signal: 'SIGINT', number: 2 })
  assert.deepEqual(await signals.next(), { signal: 'SIGTERM', number: 15 })
  const pending = assert.rejects(signals.next(), /closed/)
  signals.close()
  await pending
  assert.equal(source.listenerCount('SIGINT'), 0)
  assert.equal(source.listenerCount('SIGTERM'), 0)
})

test('cancelling a signal wait does not consume the next signal', async () => {
  const source = new EventEmitter()
  const signals = createSignals(source)
  const controller = new AbortController()
  const cancelled = assert.rejects(signals.next({ signal: controller.signal }), { name: 'AbortError' })
  controller.abort()
  await cancelled
  source.emit('SIGINT')
  assert.deepEqual(await signals.next(), { signal: 'SIGINT', number: 2 })
  signals.close()
})
