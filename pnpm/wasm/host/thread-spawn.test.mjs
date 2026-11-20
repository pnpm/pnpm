import assert from 'node:assert/strict'
import { once } from 'node:events'
import { test } from 'node:test'
import { Worker } from 'node:worker_threads'

import { createThreadSpawn } from './thread-spawn.mjs'

const cancellation = new URL('./cancellation.mjs', import.meta.url).href
const waiter = new URL('./wait-for-reply.mjs', import.meta.url).href

test('thread spawn preserves both result ABIs and allocation ownership', () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true })
  const freed = []
  let failure = 0
  const spawn = createThreadSpawn(memory, message => {
    assert.equal(message.__emnapi__.type, 'spawn-thread')
    assert.equal(message.__emnapi__.payload.startArg, 777)
    const result = new Int32Array(memory.buffer, message.__emnapi__.payload.errorOrTid, 2)
    Atomics.store(result, 0, failure)
    Atomics.store(result, 1, failure ? 6 : 43)
    Atomics.notify(result, 1)
  }, { instance: () => ({ exports: { malloc: () => 8, free: pointer => freed.push(pointer) } }) })
  assert.equal(spawn(777), 43)
  assert.deepEqual(freed, [8])
  assert.equal(spawn(777, 16), 0)
  failure = 1
  assert.equal(spawn(777), -6)
  assert.equal(spawn(777, 16), 1)
  assert.deepEqual(freed, [8, 8])
})

test('bounded RPC waits do not report a timeout and observe cancellation', async () => {
  await Promise.all([false, true].map(async cancel => {
    const state = new Int32Array(new SharedArrayBuffer(4))
    const control = new Int32Array(new SharedArrayBuffer(8))
    const worker = new Worker(`
      const { workerData, parentPort } = require('node:worker_threads');
      Promise.all([import(${JSON.stringify(waiter)}),import(${JSON.stringify(cancellation)})]).then(([{waitForReply},{createCancellationCheck}]) => {
        parentPort.postMessage('ready');
        try { waitForReply(workerData.state, 0, createCancellationCheck(workerData.control.buffer)); parentPort.postMessage('replied'); }
        catch(error) { parentPort.postMessage(error.message); }
      });
    `, { eval: true, workerData: { state, control } })
    try {
      assert.deepEqual(await once(worker, 'message'), ['ready'])
      const result = once(worker, 'message')
      await new Promise(resolve => setTimeout(resolve, 250))
      if (cancel) Atomics.store(control, 0, 1)
      else Atomics.store(state, 0, 1)
      assert.deepEqual(await result, [cancel ? 'WASM worker cancelled' : 'replied'])
    } finally {
      await worker.terminate()
    }
  }))
})
