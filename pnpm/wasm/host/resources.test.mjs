import assert from 'node:assert/strict'
import { test } from 'node:test'

import { createStream } from './resources.mjs'

test('empty stream chunks do not signal EOF to guest readers', async () => {
  const chunks = [new Uint8Array(), Uint8Array.of(1, 2, 3), new Uint8Array()]
  const stream = createStream(async () => {
    const value = chunks.shift()
    return value === undefined ? { done: true } : { done: false, value }
  }, () => {})
  assert.deepEqual(await stream.read(2), { bytes: Uint8Array.of(1, 2), done: false })
  assert.deepEqual(await stream.read(2), { bytes: Uint8Array.of(3), done: false })
  assert.deepEqual(await stream.read(2), { bytes: new Uint8Array(), done: true })
})
