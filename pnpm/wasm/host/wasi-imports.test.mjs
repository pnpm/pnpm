import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import test from 'node:test'

import { attachWasi } from './wasi-imports.mjs'

for (const [code, expected] of [['EIO', 29], ['EXDEV', 75], ['EBUSY', 10], ['ENOTEMPTY', 55], ['EOVERFLOW', 61], ['EDQUOT', 19], ['EOPNOTSUPP', 58]]) {
  test(`WASI imports return ${code} as a guest errno instead of aborting the runtime`, () => {
    const worker = new EventEmitter()
    const buffer = new SharedArrayBuffer(8)
    attachWasi(worker, { wasi_snapshot_preview1: { path_link () { throw Object.assign(new Error('filesystem operation failed'), { code }) } } })
    worker.emit('message', { pnpmWasiRpc: { namespace: 'wasi_snapshot_preview1', name: 'path_link', args: [], buffer } })
    assert.deepEqual([...new Int32Array(buffer)], [1, expected])
  })
}

test('WASI import programmer errors still fail the runtime and wake the guest', () => {
  const worker = new EventEmitter()
  const buffer = new SharedArrayBuffer(8)
  const failure = new TypeError('invalid import state')
  let reported
  worker.on('error', error => { reported = error })
  attachWasi(worker, { wasi_snapshot_preview1: { fd_filestat_get () { throw failure } } })
  worker.emit('message', { pnpmWasiRpc: { namespace: 'wasi_snapshot_preview1', name: 'fd_filestat_get', args: [], buffer } })
  assert.equal(reported, failure)
  assert.deepEqual([...new Int32Array(buffer)], [1, 29])
})

test('async WASI imports leave the guest waiting until completion', async () => {
  const worker = new EventEmitter()
  const buffer = new SharedArrayBuffer(8)
  let finish
  const pending = new Promise(resolve => { finish = resolve })
  attachWasi(worker, { wasi_snapshot_preview1: { poll_oneoff: () => pending } })
  worker.emit('message', { pnpmWasiRpc: { namespace: 'wasi_snapshot_preview1', name: 'poll_oneoff', args: [], buffer } })
  assert.equal(new Int32Array(buffer)[0], 0)
  finish(27)
  await pending
  assert.deepEqual([...new Int32Array(buffer)], [1, 27])
})
