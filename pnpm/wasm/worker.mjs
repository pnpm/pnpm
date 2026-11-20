import { parentPort, workerData } from 'node:worker_threads'

import { ThreadMessageHandler, WASIThreads } from '@emnapi/wasi-threads'

import { createCancellationCheck } from './host/cancellation.mjs'
import { createThreadSpawn } from './host/thread-spawn.mjs'

import { createGuestImports } from './host/guest-imports.mjs'
import { createAtomicWaits } from './host/atomic-waits.mjs'
import { createWasiImports, GuestExit } from './host/wasi-imports.mjs'

const checkCancelled = createCancellationCheck(workerData.atomicControl)
const postMessage = message => parentPort.postMessage(message)
let originalInstance

// Guest failures reach the supervisor through reportError. The default handler also posts a
// thread-error message, which the thread manager treats as fatal and rethrows.
class HostThreadMessageHandler extends ThreadMessageHandler {
  reportError (error) {
    reportError(error)
  }
}

const handler = new HostThreadMessageHandler({
  postMessage,
  async onLoad ({ wasmModule, wasmMemory }) {
    const threads = new WASIThreads({
      wasi: { start () {}, initialize () {} },
      childThread: true,
      postMessage,
    })
    originalInstance = await WebAssembly.instantiate(wasmModule, {
      env: { memory: wasmMemory },
      wasi_snapshot_preview1: createWasiImports(wasmModule, postMessage, 'wasi_snapshot_preview1', checkCancelled),
      pnpm_fs: createWasiImports(wasmModule, postMessage, 'pnpm_fs', checkCancelled),
      pnpm_host: createGuestImports(wasmMemory, postMessage, checkCancelled),
      pnpm_atomic: createAtomicWaits(wasmMemory, workerData.atomicControl, checkCancelled),
      wasi: { 'thread-spawn': createThreadSpawn(wasmMemory, postMessage, {
        instance: () => originalInstance, checkCancelled,
      }) },
    })
    return { module: wasmModule, instance: threads.initialize(originalInstance, wasmModule, wasmMemory) }
  },
})

parentPort.on('message', message => {
  if (!message.pnpmMainStart) {
    handler.handle({ data: message })
    return
  }
  try {
    // The first instance initializes shared memory; each instance has its own TLS base global.
    originalInstance.exports.__wasm_init_tls(message.tlsBase)
    originalInstance.exports._start()
    postMessage({ pnpmMainExit: 0 })
  } catch (error) {
    reportError(error)
  }
})

function reportError (error) {
  if (error instanceof GuestExit) postMessage({ pnpmMainExit: error.code })
  else postMessage({ pnpmMainError: { name: error.name, message: error.message, stack: error.stack } })
}
