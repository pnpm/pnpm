import { isFetchTimeoutError } from '@pnpm/error'

/**
 * In-flight cap for the requests one {@link createFetchFromRegistry} client
 * sends to a single origin.
 *
 * The cap starts unbounded. `networkConcurrency` (the package-requester
 * queue and `maxSockets`) remains the fast-path limit. After a fetch
 * timeout while another request to the same origin is still running, the
 * cap drops to one so a slow link stops timing out its own downloads.
 * Other origins keep their own gates.
 */
export interface NetworkConcurrencyGate {
  readonly limit: number
  acquire: () => Promise<void>
  release: () => void
  /**
   * Shrink the cap to one connection. The caller must still hold the
   * permit it acquired. Returns whether the cap changed.
   */
  downscaleIfPeersActive: () => boolean
}

export type GetOriginConcurrencyGate = (origin: string) => NetworkConcurrencyGate

export function createOriginConcurrencyGates (): GetOriginConcurrencyGate {
  const gates = new Map<string, NetworkConcurrencyGate>()
  return (origin) => {
    let gate = gates.get(origin)
    if (gate == null) {
      gate = createNetworkConcurrencyGate()
      gates.set(origin, gate)
    }
    return gate
  }
}

export function createNetworkConcurrencyGate (limit: number = Number.POSITIVE_INFINITY): NetworkConcurrencyGate {
  let currentLimit = limit
  let inFlight = 0
  const waiters: Array<() => void> = []

  return {
    get limit () {
      return currentLimit
    },
    async acquire () {
      if (inFlight < currentLimit) {
        inFlight++
        return
      }
      await new Promise<void>((resolve) => {
        waiters.push(resolve)
      })
    },
    release () {
      if (inFlight === 0) return
      inFlight--
      if (inFlight < currentLimit) {
        const resolve = waiters.shift()
        if (resolve != null) {
          inFlight++
          resolve()
        }
      }
    },
    downscaleIfPeersActive () {
      if (currentLimit <= 1 || inFlight <= 1) return false
      currentLimit = 1
      return true
    },
  }
}

const BODY_READERS = new Set<string | symbol>(['text', 'json', 'arrayBuffer', 'blob'])

/**
 * Keeps the concurrency permit until the caller finishes the body, so a
 * stalled download still counts as in flight. `text` / `json` /
 * `arrayBuffer` / `blob` read the original body and then release. A body
 * that nobody touches releases on the next turn so a HEAD or an abandoned
 * redirect cannot pin the slot.
 */
export function holdPermitUntilBodySettles (res: Response, gate: NetworkConcurrencyGate): Response {
  const body = res.body
  if (body == null) {
    gate.release()
    return res
  }
  const permit = holdPermit(gate)
  const tracked = trackBody(body, permit)
  return new Proxy(res, {
    get (target, prop) {
      if (prop === 'body') {
        permit.keep()
        return tracked
      }
      if (BODY_READERS.has(prop)) return readAndRelease(target, prop, permit)
      // Undici stores response state in private fields, which reject a proxy receiver.
      const value: unknown = Reflect.get(target, prop, target)
      return typeof value === 'function' ? value.bind(target) : value
    },
  })
}

interface HeldPermit {
  /** Cancel the next-turn release because a reader took the body. */
  keep: () => void
  /** Release once. A fetch timeout first lowers the cap to one. */
  release: (error?: unknown) => void
}

function holdPermit (gate: NetworkConcurrencyGate): HeldPermit {
  let released = false
  const timer = setTimeout(release, 0)
  function release (error?: unknown): void {
    if (released) return
    released = true
    clearTimeout(timer)
    if (isFetchTimeoutError(error)) gate.downscaleIfPeersActive()
    gate.release()
  }
  return {
    keep: () => {
      clearTimeout(timer)
    },
    release,
  }
}

function trackBody (body: ReadableStream<Uint8Array>, permit: HeldPermit): ReadableStream<Uint8Array> {
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
  // Node fills a default highWaterMark of 1 as soon as the stream exists,
  // which would lock the original body before text() or json() can read it.
  return new ReadableStream<Uint8Array>({
    async pull (controller) {
      try {
        reader ??= body.getReader()
        const { done, value } = await reader.read()
        if (done) {
          permit.release()
          controller.close()
          return
        }
        controller.enqueue(value)
      } catch (error: unknown) {
        permit.release(error)
        controller.error(error)
      }
    },
    cancel (reason) {
      permit.release(reason)
      return reader != null ? reader.cancel(reason) : body.cancel(reason)
    },
  }, { highWaterMark: 0 })
}

function readAndRelease (target: Response, prop: string | symbol, permit: HeldPermit): () => Promise<unknown> {
  return async () => {
    permit.keep()
    try {
      const read = Reflect.get(target, prop, target) as () => Promise<unknown>
      return await read.call(target)
    } catch (error: unknown) {
      permit.release(error)
      throw error
    } finally {
      permit.release()
    }
  }
}
