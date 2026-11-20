export function createResources () {
  const resources = new Map()
  let nextHandle = 1
  let closed = false
  return {
    add (resource) {
      if (closed) throw new Error('WASM host resources are closed')
      if (nextHandle > 0x7fffffff) throw new Error('WASM host resource handles exhausted')
      const handle = nextHandle++
      resources.set(handle, resource)
      return handle
    },
    get (handle, kind) {
      const resource = resources.get(handle)
      if (!resource || resource.kind !== kind) throw new Error(`Invalid ${kind} handle: ${handle}`)
      return resource
    },
    findProcess (pid) {
      const resource = [...resources.values()].find(resource => resource.kind === 'process' && resource.child.pid === pid)
      if (!resource) throw Object.assign(new Error(`No owned process with PID ${pid}`), { code: 'ESRCH' })
      return resource
    },
    remove (handle) {
      resources.delete(handle)
    },
    async close (handle) {
      const resource = resources.get(handle)
      if (!resource) throw new Error(`Invalid resource handle: ${handle}`)
      resources.delete(handle)
      await resource.close()
    },
    async closeAll () {
      closed = true
      const pending = [...resources.values()]
      resources.clear()
      const results = await Promise.allSettled(pending.map(resource => resource.close()))
      const errors = results.filter(result => result.status === 'rejected').map(result => result.reason)
      if (errors.length) throw new AggregateError(errors, 'Failed to close WASM host resources')
    },
  }
}

export function createStream (reader, cancel) {
  let remainder = new Uint8Array()
  let reading = false
  let closed = false
  return {
    kind: 'stream',
    async read (maxBytes = 65536) {
      if (!Number.isInteger(maxBytes) || maxBytes < 1 || maxBytes > 65536) {
        throw new RangeError('Stream read size must be between 1 and 65536')
      }
      if (reading) throw new Error('Concurrent reads of one stream are not supported')
      if (closed) throw new Error('Stream is closed')
      reading = true
      try {
        while (remainder.length === 0) {
          // eslint-disable-next-line no-await-in-loop -- Empty chunks must be consumed before returning stream bytes.
          const result = await reader()
          if (result.done) return { bytes: new Uint8Array(), done: true }
          remainder = result.value
        }
        const bytes = Uint8Array.from(remainder.subarray(0, maxBytes))
        remainder = remainder.subarray(bytes.length)
        return { bytes, done: false }
      } finally {
        reading = false
      }
    },
    async close () {
      closed = true
      remainder = new Uint8Array()
      await cancel()
    },
  }
}
