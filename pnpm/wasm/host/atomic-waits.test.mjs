import assert from 'node:assert/strict'
import { once } from 'node:events'
import { test } from 'node:test'
import { Worker } from 'node:worker_threads'

import { encodeWat } from '../encode-wat.mjs'
import { instrumentAtomicWaits } from '../instrument-atomics.mjs'
import { cancelAtomicWaits, createAtomicWaits } from './atomic-waits.mjs'

test('atomic wait instrumentation preserves offsets, return values and function references', async () => {
  const source = encodeWat(`(module
    (import "env" "memory" (memory 1 2 shared))
    (func $value (result i32) i32.const 42)
    (func (export "value") (result i32) call $value)
    (func (export "wait32") (param i32 i32 i64) (result i32)
      local.get 0 local.get 1 local.get 2 memory.atomic.wait32 offset=4)
    (func (export "wait64") (param i32 i64 i64) (result i32)
      local.get 0 local.get 1 local.get 2 memory.atomic.wait64 offset=8))`)
  {
    const bytes = await instrumentAtomicWaits(source)
    assert.deepEqual(await instrumentAtomicWaits(bytes), bytes)
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true })
    const imports = createAtomicWaits(memory, new SharedArrayBuffer(8))
    const { instance } = await WebAssembly.instantiate(bytes, { env: { memory }, pnpm_atomic: imports })
    assert.equal(instance.exports.value(), 42)
    assert.equal(instance.exports.wait32(0, 1, -1n), 1)
    assert.equal(instance.exports.wait64(0, 1n, -1n), 1)
    assert.equal(instance.exports.wait32(0, 0, 1000000n), 2)
    assert.throws(() => instance.exports.wait32(-4, 0, 0n), WebAssembly.RuntimeError)
    assert.throws(() => instance.exports.wait64(1, 0n, 0n), WebAssembly.RuntimeError)
  }
})

test('cancellation wakes an indefinitely waiting worker', { timeout: 3000 }, async t => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true })
  const control = new SharedArrayBuffer(8)
  const moduleUrl = new URL('./atomic-waits.mjs', import.meta.url).href
  const worker = new Worker(`
    const { parentPort, workerData } = require('node:worker_threads');
    import(workerData.moduleUrl).then(({ createAtomicWaits }) => {
      parentPort.postMessage('ready');
      try { createAtomicWaits(workerData.memory, workerData.control).wait32(0, 0, -1n, 0); }
      catch (error) { parentPort.postMessage(error.message); }
    });
  `, { eval: true, workerData: { memory, control, moduleUrl } })
  t.after(() => worker.terminate())
  assert.deepEqual(await once(worker, 'message'), ['ready'])
  const cancelled = once(worker, 'message')
  cancelAtomicWaits(memory, control)
  assert.deepEqual(await cancelled, ['WASM worker cancelled'])
})
