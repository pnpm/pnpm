/**
 * In-flight cap for registry requests that share one
 * {@link createFetchFromRegistry} client.
 *
 * The cap starts unbounded. `networkConcurrency` (the package-requester
 * queue and `maxSockets`) remains the fast-path limit. After a fetch
 * timeout while another request is still running, the cap drops to one
 * so a slow link stops timing out its own downloads.
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
