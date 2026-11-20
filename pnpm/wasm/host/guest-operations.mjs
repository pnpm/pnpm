import { createHostServices } from './operations.mjs'
import { RESPONSE_CAPACITY } from './protocol.mjs'

export function createGuestOperations (services = createHostServices()) {
  const pending = new Map()
  const completed = []
  const waiters = []
  const inFlight = new Set()
  const cleanupErrors = []
  let nextId = 1
  let closed = false

  function start (request) {
    if (closed || nextId > 0x7fffffff) return -2
    const id = nextId++
    const entry = { controller: new AbortController(), response: null, operation: request.operation, result: null }
    pending.set(id, entry)
    if (request.operation === 'process.write') request.bytes = Uint8Array.from(request.bytes)
    if (request.operation === 'network.request' && Array.isArray(request.body)) request.body = Uint8Array.from(request.body)
    const task = Promise.resolve().then(() => services.dispatch(request, { signal: entry.controller.signal })).then(
      value => settle(id, entry, { ok: true, value }),
      error => settle(id, entry, { ok: false, error: { message: error.message, code: errorCode(error) } }),
    )
    track(task)
    return id
  }

  function track (task) {
    inFlight.add(task)
    task.then(() => inFlight.delete(task), error => {
      inFlight.delete(task)
      cleanupErrors.push(error)
    })
  }

  async function releaseResult (operation, result) {
    if (!result?.ok) return
    const handles = ['network.request', 'upload.create', 'terminal.open'].includes(operation) ? [result.value.handle]
      : ['process.spawn', 'shell.spawn'].includes(operation) ? [result.value.stdout, result.value.stderr, result.value.handle] : []
    await Promise.all(handles.filter(handle => handle != null)
      .map(handle => services.dispatch({ operation: 'resource.close', handle })))
  }

  async function settle (id, entry, result) {
    if (!pending.has(id)) return closed ? undefined : releaseResult(entry.operation, result)
    let bytes
    try {
      bytes = new TextEncoder().encode(JSON.stringify(result, (_key, value) => value instanceof Uint8Array ? [...value] : value))
      if (bytes.length > RESPONSE_CAPACITY) throw new Error('WASM host response exceeds the transfer limit')
    } catch (error) {
      try {
        await releaseResult(entry.operation, result)
      } catch (cleanupError) {
        cleanupErrors.push(cleanupError)
      }
      result = { ok: false, error: { message: error.message } }
      bytes = new TextEncoder().encode(JSON.stringify(result))
    }
    if (!pending.has(id)) return
    entry.result = result
    entry.response = bytes
    const waiter = waiters.shift()
    if (waiter) waiter(id)
    else completed.push(id)
  }

  async function dispatch (method, args) {
    switch (method) {
    case 'start': return start(args[0])
    case 'length': return pending.get(args[0])?.response?.length ?? (pending.has(args[0]) ? -1 : -2)
    case 'read': {
      const entry = pending.get(args[0])
      if (!entry?.response || entry.response.length > args[1]) return -2
      pending.delete(args[0])
      return entry.response
    }
    case 'cancel': {
      const entry = pending.get(args[0])
      pending.delete(args[0])
      entry?.controller.abort()
      if (entry?.result) await releaseResult(entry.operation, entry.result)
      return 0
    }
    case 'close-resource': {
      await services.dispatch({ operation: 'resource.close', handle: args[0] })
      return 0
    }
    case 'wait': {
      while (completed.length > 0) {
        const id = completed.shift()
        if (pending.has(id)) return id
      }
      return closed ? 0 : new Promise(resolve => waiters.push(resolve))
    }
    default: throw new Error(`Unknown guest operation: ${method}`)
    }
  }

  return {
    dispatch,
    attach (worker) {
      const listener = message => {
        if (!message.pnpmHostRpc) return
        const { method, args, buffer } = message.pnpmHostRpc
        respond(buffer, dispatch(method, args)).catch(error => worker.emit('error', error))
      }
      worker.on('message', listener)
      return () => worker.off('message', listener)
    },
    async close () {
      closed = true
      for (const entry of pending.values()) entry.controller.abort()
      pending.clear()
      for (const waiter of waiters.splice(0)) waiter(0)
      try {
        await services.close()
      } finally {
        await Promise.allSettled([...inFlight])
      }
      if (cleanupErrors.length) throw new AggregateError(cleanupErrors, 'Failed to clean up cancelled WASM operations')
    },
  }
}

async function respond (buffer, response) {
  const header = new Int32Array(buffer, 0, 3)
  Atomics.store(header, 1, -2)
  Atomics.store(header, 2, 0)
  try {
    const value = await response
    if (value instanceof Uint8Array) {
      new Uint8Array(buffer, 12).set(value)
      Atomics.store(header, 1, value.length)
      Atomics.store(header, 2, value.length)
    } else {
      Atomics.store(header, 1, value)
      Atomics.store(header, 2, 0)
    }
  } finally {
    Atomics.store(header, 0, 1)
    Atomics.notify(header, 0)
  }
}

function errorCode (error) {
  const code = error.code ?? error.cause?.code
  return typeof code === 'string' ? code : undefined
}
