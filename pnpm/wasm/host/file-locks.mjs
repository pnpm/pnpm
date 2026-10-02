import { fstatSync } from 'node:fs'

import { acquireFileLease } from './lock-registry.mjs'

const busy = 6

export function createFileLocks () {
  const held = new Map()
  function release (descriptor) {
    const state = held.get(descriptor)
    if (!state) return
    held.delete(descriptor)
    state.closed = true
    state.lease?.release()
  }
  return {
    tryLock (descriptor, exclusive) {
      let state = held.get(descriptor)
      if (!state) {
        const metadata = fstatSync(descriptor, { bigint: true })
        state = { identity: `${metadata.dev}:${metadata.ino}`, closed: false }
        held.set(descriptor, state)
      }
      if (state.error) return 29
      if (state.lease?.exclusive === exclusive) return 0
      if (!state.pending) {
        state.pending = true
        acquireFileLease(state.identity, exclusive, state.lease).then(lease => {
          if (state.closed) lease?.release()
          else if (lease) state.lease = lease
        }, error => { state.error = error }).finally(() => { state.pending = false })
      }
      return busy
    },
    release,
    close () {
      for (const descriptor of held.keys()) release(descriptor)
    },
  }
}
