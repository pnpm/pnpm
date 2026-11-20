import { createCancellationCheck } from './cancellation.mjs'

// Cancellation can race the transition from checking its flag to waiting on guest memory.
const WAIT_SLICE_MS = 100

export function createAtomicWaits (memory, control, checkCancelled = createCancellationCheck(control)) {
  const state = new Int32Array(control)
  if (state.length < 2) throw new RangeError('Atomic wait control requires two i32 slots')
  return {
    wait32: (pointer, expected, timeout, offset) => wait(memory, state, pointer, expected, timeout, offset, 4, checkCancelled),
    wait64: (pointer, expected, timeout, offset) => wait(memory, state, pointer, expected, timeout, offset, 8, checkCancelled),
  }
}

export function cancelAtomicWaits (memory, control) {
  const state = new Int32Array(control)
  Atomics.store(state, 0, 1)
  const address = Atomics.load(state, 1) >>> 0
  if (address < memory.buffer.byteLength) Atomics.notify(new Int32Array(memory.buffer), address >>> 2)
}

function wait (memory, state, pointer, expected, timeout, offset, width, checkCancelled) {
  const address = (pointer >>> 0) + (offset >>> 0)
  if (address % width !== 0 || address + width > memory.buffer.byteLength) {
    throw new WebAssembly.RuntimeError('Unaligned or out-of-bounds atomic wait')
  }
  const view = width === 4 ? new Int32Array(memory.buffer) : new BigInt64Array(memory.buffer)
  const deadline = timeout < 0n ? Infinity : performance.now() + Number(timeout) / 1000000
  Atomics.store(state, 1, address)
  try {
    while (true) {
      checkCancelled()
      const remaining = Math.max(0, deadline - performance.now())
      const result = Atomics.wait(view, address / width, expected, Math.min(remaining, WAIT_SLICE_MS))
      checkCancelled()
      if (result === 'ok') return 0
      if (result === 'not-equal') return 1
      if (performance.now() >= deadline) return 2
    }
  } finally {
    Atomics.store(state, 1, -1)
  }
}
