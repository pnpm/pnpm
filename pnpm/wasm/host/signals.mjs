export function createSignals (source = process) {
  const queued = []
  const waiters = []
  let handlers
  let closed = false

  function subscribe () {
    if (handlers) return
    handlers = ['SIGINT', 'SIGTERM'].map(signal => {
      const listener = () => {
        const value = { signal, number: signal === 'SIGINT' ? 2 : 15 }
        const waiter = waiters.shift()
        if (waiter) waiter.resolve(value)
        else queued.push(value)
      }
      source.on(signal, listener)
      return [signal, listener]
    })
  }

  return {
    next ({ signal } = {}) {
      if (closed) return Promise.reject(new Error('Signal subscription is closed'))
      signal?.throwIfAborted()
      subscribe()
      if (queued.length) return Promise.resolve(queued.shift())
      return new Promise((resolve, reject) => {
        const abort = () => {
          const index = waiters.indexOf(waiter)
          if (index !== -1) waiters.splice(index, 1)
          waiter.reject(signal.reason)
        }
        const finish = callback => value => {
          signal?.removeEventListener('abort', abort)
          callback(value)
        }
        const waiter = { resolve: finish(resolve), reject: finish(reject) }
        waiters.push(waiter)
        signal?.addEventListener('abort', abort, { once: true })
      })
    },
    close () {
      closed = true
      for (const [signal, listener] of handlers ?? []) source.off(signal, listener)
      for (const waiter of waiters.splice(0)) waiter.reject(new Error('Signal subscription is closed'))
      queued.length = 0
    },
  }
}
