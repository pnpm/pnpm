import { waitForReply } from './wait-for-reply.mjs'

import { errno } from './wasi-errno.mjs'

const { WebAssembly } = globalThis

export class GuestExit extends Error {
  constructor (code) {
    super(`WASI guest exited with status ${code}`)
    this.code = code
  }
}

export function createWasiImports (module, postMessage, namespace = 'wasi_snapshot_preview1', checkCancelled) {
  const buffer = new SharedArrayBuffer(8)
  const state = new Int32Array(buffer)
  return Object.fromEntries(WebAssembly.Module.imports(module)
    .filter(entry => entry.module === namespace)
    .map(({ name }) => [name, (...args) => {
      if (namespace === 'wasi_snapshot_preview1' && name === 'proc_exit') {
        postMessage({ pnpmMainExit: args[0] })
        throw new GuestExit(args[0])
      }
      Atomics.store(state, 0, 0)
      postMessage({ pnpmWasiRpc: { namespace, name, args, buffer } })
      waitForReply(state, 0, checkCancelled)
      return Atomics.load(state, 1)
    }]))
}

export function attachWasi (worker, imports) {
  worker.on('message', message => {
    if (!message.pnpmWasiRpc) return
    const { namespace, name, args, buffer } = message.pnpmWasiRpc
    const state = new Int32Array(buffer)
    Atomics.store(state, 1, 29)
    function complete (result, error) {
      try {
        if (!error) Atomics.store(state, 1, result)
        else {
          const code = namespace === 'wasi_snapshot_preview1' && errno[error?.code]
          if (typeof code === 'number') Atomics.store(state, 1, code)
          else worker.emit('error', error)
        }
      } finally {
        Atomics.store(state, 0, 1)
        Atomics.notify(state, 0)
      }
    }
    try {
      const result = imports[namespace][name](...args)
      if (typeof result?.then === 'function') result.then(value => complete(value), error => complete(undefined, error))
      else complete(result)
    } catch (error) {
      complete(undefined, error)
    }
  })
}
