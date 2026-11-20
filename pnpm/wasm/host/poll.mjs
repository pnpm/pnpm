import { performance } from 'node:perf_hooks'
import { setTimeout } from 'node:timers/promises'

import { errno } from './wasi-errno.mjs'

const { AbortController } = globalThis

// Clock polling must yield the supervisor so other guest threads, leases and
// process termination remain responsive while a Rust thread sleeps.
export function createPoll (memory) {
  const controller = new AbortController()
  return { close () { controller.abort() }, async poll (input, output, count, written) {
    try {
      if (!input || !output || !count || !written) return errno.EINVAL
      const subscriptions = readSubscriptions(memory, input >>> 0, count >>> 0)
      new Uint8Array(memory.buffer, output >>> 0, (count >>> 0) * 32)
      new DataView(memory.buffer).setUint32(written >>> 0, 0, true)
      const descriptors = subscriptions.filter(subscription => subscription.type !== 0)
      if (descriptors.length) return writeEvents(memory, { output, written }, descriptors, errno.ENOSYS)
      const first = subscriptions.reduce((earliest, entry) => entry.delay < earliest.delay ? entry : earliest)
      await waitForClock(first.delay, controller.signal)
      return writeEvents(memory, { output, written }, [first], 0)
    } catch (error) {
      if (error.name === 'AbortError') return errno.EINTR
      if (error instanceof RangeError) return errno.EFAULT
      if (error.code === 'EINVAL') return errno.EINVAL
      throw error
    }
  } }
}

function readSubscriptions (memory, input, count) {
  const view = new DataView(memory.buffer, input, count * 48)
  return Array.from({ length: count }, (_, index) => {
    const offset = index * 48
    const type = view.getUint8(offset + 8)
    if (type > 2) throw Object.assign(new Error('Invalid WASI subscription type'), { code: 'EINVAL' })
    return { userdata: view.getBigUint64(offset, true), type, delay: type === 0 ? clockDelay(view, offset) : 0n }
  })
}

function clockDelay (view, offset) {
  const clock = view.getUint32(offset + 16, true)
  const timeout = view.getBigUint64(offset + 24, true)
  const flags = view.getUint16(offset + 40, true)
  if (clock > 3 || flags > 1) throw Object.assign(new Error('Invalid WASI clock subscription'), { code: 'EINVAL' })
  if (flags === 0) return timeout
  // Match the clock epoch used by the injected WASI clock_time_get import.
  const now = BigInt(Math.floor(clock === 0 ? Date.now() : performance.now())) * 1000000n
  return timeout > now ? timeout - now : 0n
}

async function waitForClock (delay, signal) {
  const deadline = performance.now() + Number(delay) / 1000000
  let remaining = deadline - performance.now()
  do {
    // eslint-disable-next-line no-await-in-loop -- Recheck the deadline after early or maximum-length timers.
    await setTimeout(Math.min(Math.ceil(Math.max(remaining, 0)), 2147483647), undefined, { signal })
    remaining = deadline - performance.now()
  } while (remaining > 0)
}

function writeEvents (memory, { output, written }, subscriptions, error) {
  const view = new DataView(memory.buffer)
  subscriptions.forEach((subscription, index) => {
    const offset = (output >>> 0) + index * 32
    new Uint8Array(memory.buffer, offset, 32).fill(0)
    view.setBigUint64(offset, subscription.userdata, true)
    view.setUint16(offset + 8, error, true)
    view.setUint8(offset + 10, subscription.type)
  })
  view.setUint32(written >>> 0, subscriptions.length, true)
  return 0
}
