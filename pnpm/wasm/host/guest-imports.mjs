import { RESPONSE_CAPACITY } from './protocol.mjs'
import { waitForReply } from './wait-for-reply.mjs'

export function createGuestImports (memory, postMessage, checkCancelled) {
  const buffer = new SharedArrayBuffer(RESPONSE_CAPACITY + 12)
  const header = new Int32Array(buffer, 0, 3)

  function call (method, args) {
    Atomics.store(header, 0, 0)
    postMessage({ pnpmHostRpc: { method, args, buffer } })
    waitForReply(header, 0, checkCancelled)
    return Atomics.load(header, 1)
  }

  function view (pointer, length) {
    return new Uint8Array(memory.buffer, pointer >>> 0, length >>> 0)
  }

  return {
    operation_start (pointer, length) {
      if ((length >>> 0) > RESPONSE_CAPACITY) return -2
      const bytes = Uint8Array.from(view(pointer, length))
      const request = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes))
      return call('start', [request])
    },
    response_len (id) {
      return call('length', [id])
    },
    response_read (id, pointer, length) {
      const target = view(pointer, length)
      const result = call('read', [id, target.length])
      if (result >= 0) target.set(new Uint8Array(buffer, 12, Atomics.load(header, 2)))
      return result
    },
    operation_cancel (id) {
      call('cancel', [id])
    },
    resource_close (handle) {
      return call('close-resource', [handle])
    },
    wait_completion () {
      return call('wait', [])
    },
  }
}
