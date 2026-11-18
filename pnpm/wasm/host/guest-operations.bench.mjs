import assert from 'node:assert/strict'
import { Worker } from 'node:worker_threads'

import { createGuestOperations } from './guest-operations.mjs'

const payload = Uint8Array.from({ length: 64 * 1024 }, (_, index) => index & 0xff)
const services = createGuestOperations({
  async dispatch (request) {
    if (request.operation === 'stream.read') return { done: false, bytes: payload }
    return { bytes: payload }
  },
  async close () {},
})
const importsUrl = new URL('./guest-imports.mjs', import.meta.url).href
const workerSource = `
  const { parentPort } = require('node:worker_threads')
  ;(async () => {
    const { createGuestImports } = await import(${JSON.stringify(importsUrl)})
    const memory = new WebAssembly.Memory({ initial: 5 })
    const imports = createGuestImports(memory, message => parentPort.postMessage(message))
    const samples = { json: [], binary: [] }
    function transfer (operation) {
      const request = new TextEncoder().encode(JSON.stringify({ operation }))
      new Uint8Array(memory.buffer).set(request)
      const id = imports.operation_start(0, request.length)
      imports.wait_completion()
      const responseLength = imports.response_len(id)
      if (operation === 'stream.read') {
        imports.response_read(id, 1024, responseLength)
        const response = JSON.parse(new TextDecoder().decode(new Uint8Array(memory.buffer, 1024, responseLength)))
        if (response.binaryLength !== 65536) throw new Error('Unexpected binary length')
        imports.response_binary_read(id, 4096, response.binaryLength)
      } else {
        const copied = imports.response_read(id, 1024, responseLength)
        const response = JSON.parse(new TextDecoder().decode(new Uint8Array(memory.buffer, 1024, copied)))
        if (response.value.bytes.length !== 65536) throw new Error('Unexpected JSON byte length')
      }
    }
    for (let index = 0; index < 10; index++) {
      transfer('json')
      transfer('stream.read')
    }
    for (let index = 0; index < 100; index++) {
      const order = index % 2 ? ['stream.read', 'json'] : ['json', 'stream.read']
      for (const operation of order) {
        const started = performance.now()
        transfer(operation)
        samples[operation === 'stream.read' ? 'binary' : 'json'].push((performance.now() - started) * 1000)
      }
    }
    parentPort.postMessage({ benchmarkResult: samples })
  })().catch(error => { throw error })
`
const worker = new Worker(workerSource, { eval: true })
services.attach(worker)
try {
  const samples = await new Promise((resolve, reject) => {
    worker.once('error', reject)
    worker.on('message', message => {
      if (message.benchmarkResult) resolve(message.benchmarkResult)
    })
  })
  for (const values of Object.values(samples)) assert.equal(values.length, 100)
  const jsonLength = new TextEncoder().encode(JSON.stringify({ ok: true, value: { bytes: [...payload] } })).length
  const binaryControlLength = new TextEncoder().encode(JSON.stringify({ ok: true, value: { done: false }, binaryLength: payload.length })).length
  console.log(`node=${process.version} payload=${payload.length}B rounds=100`)
  console.log(`control: JSON=${jsonLength}B binary=${binaryControlLength}B`)
  for (const [name, values] of Object.entries(samples)) {
    values.sort((left, right) => left - right)
    console.log(`${name}: median=${values[50].toFixed(2)}us p95=${values[94].toFixed(2)}us`)
  }
} finally {
  await worker.terminate()
  await services.close()
}
