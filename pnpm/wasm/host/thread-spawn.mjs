import { waitForReply } from './wait-for-reply.mjs'

// Keep the upstream supervisor's spawn protocol while making the child wait interruptible.
export function createThreadSpawn (memory, postMessage, options) {
  return (startArg, errorOrTid) => {
    const ownsResult = errorOrTid === undefined
    const exports = options.instance().exports
    if (ownsResult) errorOrTid = exports.malloc(8) >>> 0
    if (ownsResult && !errorOrTid) return -48
    const result = new Int32Array(memory.buffer, errorOrTid, 2)
    Atomics.store(result, 0, 0)
    Atomics.store(result, 1, 0)
    postMessage({ __emnapi__: { type: 'spawn-thread', payload: { startArg, errorOrTid } } })
    waitForReply(result, 1, options.checkCancelled)
    const failed = Atomics.load(result, 0)
    const thread = Atomics.load(result, 1)
    if (!ownsResult) return failed
    exports.free(errorOrTid)
    return failed ? -thread : thread
  }
}
