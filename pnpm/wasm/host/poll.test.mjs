import assert from 'node:assert/strict'
import { performance } from 'node:perf_hooks'
import test from 'node:test'
import { setTimeout } from 'node:timers/promises'

import { createPoll } from './poll.mjs'

const { WebAssembly } = globalThis

function fixture (context) {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true })
  const polling = createPoll(memory)
  context.after(() => polling.close())
  const view = new DataView(memory.buffer)
  function clock (index, delay, { clock = 1, flags = 0 } = {}) {
    const offset = 64 + index * 48
    view.setBigUint64(offset, BigInt(index + 100), true)
    view.setUint32(offset + 16, clock, true)
    view.setBigUint64(offset + 24, delay, true)
    view.setUint16(offset + 40, flags, true)
  }
  return { ...polling, view, clock, memory }
}

test('clock polling yields the supervisor and returns only after the deadline', async context => {
  const { poll, view, clock } = fixture(context)
  clock(0, 30000000n)
  let completed = false
  const started = performance.now()
  const pending = poll(64, 512, 1, 1024).then(result => { completed = true; return result })
  await setTimeout(5)
  assert.equal(completed, false)
  assert.equal(await pending, 0)
  assert.ok(performance.now() - started >= 30)
  assert.equal(view.getUint32(1024, true), 1)
  assert.equal(view.getBigUint64(512, true), 100n)
  assert.equal(view.getUint16(520, true), 0)
  assert.equal(view.getUint8(522), 0)
})

test('earliest clock and absolute monotonic time use the host clock epoch', async context => {
  const { poll, view, clock } = fixture(context)
  clock(0, 10000000000n)
  clock(1, BigInt(Math.floor(performance.now())) * 1000000n, { flags: 1 })
  assert.equal(await poll(64, 512, 2, 1024), 0)
  assert.equal(view.getBigUint64(512, true), 101n)
})

test('shutdown interrupts pending polls without retaining their timers', async context => {
  const { poll, clock, close } = fixture(context)
  clock(0, 30000000000n)
  const pending = poll(64, 512, 1, 1024)
  close()
  assert.equal(await pending, 27)
})

test('unsupported fd polling returns an event error without waiting for a clock', async context => {
  const { poll, view, clock } = fixture(context)
  clock(0, 30000000000n)
  view.setBigUint64(112, 201n, true)
  view.setUint8(120, 1)
  assert.equal(await poll(64, 512, 2, 1024), 0)
  assert.equal(view.getBigUint64(512, true), 201n)
  assert.equal(view.getUint16(520, true), 52)
  assert.equal(view.getUint8(522), 1)
})

test('invalid subscription types and output bounds return WASI errors', async context => {
  const { poll, view, clock } = fixture(context)
  clock(0, 0n)
  assert.equal(await poll(64, 65520, 1, 1024), 21)
  view.setUint8(72, 3)
  assert.equal(await poll(64, 512, 1, 1024), 28)
  assert.equal(await poll(64, 512, 0, 1024), 28)
})
