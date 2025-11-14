import assert from 'assert'
import util from 'util'
import os from 'os'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { operation, type RetryTimeoutOptions } from '@zkochan/retry'
import nodeFetch, { type Request, type RequestInit as NodeRequestInit, Response } from 'node-fetch'

export { isRedirect } from 'node-fetch'
export { Response, type RetryTimeoutOptions }

interface URLLike { href: string }
export type RequestInfo = string | URLLike | Request

export interface RequestInit extends NodeRequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
}

/**
 * ---------------------------
 * Adaptive concurrency limiter
 * ---------------------------
 * - Two pools: 'meta' (small/latency sensitive) and 'tar' (big downloads)
 * - Adjusts every T seconds using AIMD:
 *   * Increase slowly when throughput improves & errors low
 *   * Decrease multiplicatively on errors/CPU pressure/latency growth
 * - Uses headers (content-length) to estimate bytes without consuming the body
 */

type Pool = 'meta' | 'tar'

interface FinishSignal {
  bytes: number
  error: boolean
  durationMs: number
  status?: number
}

class AdaptiveLimiter {
  private limits: Record<Pool, number>
  private active: Record<Pool, number> = { meta: 0, tar: 0 }
  private readonly queues: Record<Pool, Array<() => void>> = { meta: [], tar: [] }

  // rolling stats
  private bytesWindow = 0
  private lastBytesWindow = 0
  private readonly rollingBytesWindow: number[] = [] // Keep last 5 measurements
  private successes = 0
  private failures = 0
  private p95MetaLatencyMs = 200
  private readonly metaLatSamples: number[] = []

  // caps
  private readonly hardCap: number
  private readonly fdCap: number
  private readonly cpuCap: number

  // tick timer
  private readonly interval: NodeJS.Timeout

  constructor (opts?: { cores?: number, hardCap?: number, fdCap?: number, tickMs?: number }) {
    const cores = Math.max(1, opts?.cores ?? os.cpus().length)
    this.hardCap = Math.max(8, opts?.hardCap ?? 64)
    this.fdCap = Math.max(8, opts?.fdCap ?? 128)
    this.cpuCap = Math.max(4, 2 * cores)

    // Initial guesses
    this.limits = {
      meta: Math.min(4, cores), // keep meta small
      tar: Math.max(4, Math.min(16, 2 * cores)), // start moderate
    }

    const tickMs = Math.max(500, opts?.tickMs ?? 1000)
    this.interval = setInterval(() => {
      this.tick()
    }, tickMs)
    // Don’t keep the process open just for the timer
    this.interval.unref?.()
  }

  public getConcurrency (pool: Pool) {
    return this.limits[pool]
  }

  public activeCount (pool: Pool) {
    return this.active[pool]
  }

  /**
   * Convenience for fetch: lets us record bytes & latency without consuming body.
   */
  public async runFetch (pool: Pool, url: string, doFetch: () => Promise<Response>): Promise<Response> {
    await this.waitForSlot(pool)
    const start = Date.now()
    let res: Response | undefined
    try {
      res = await doFetch()
      // Estimate bytes from headers when possible (don’t drain the body here)
      const cl = res.headers.get('content-length')
      const bytes = cl ? Number(cl) || 0 : this.guessBytesFromUrl(url)
      this.onFinish(pool, { bytes, error: false, durationMs: Date.now() - start, status: res.status })
      return res
    } catch (error: unknown) {
      this.onFinish(pool, { bytes: 0, error: true, durationMs: Date.now() - start })
      throw error
    } finally {
      this.release(pool)
    }
  }

  private guessBytesFromUrl (u: string): number {
    // crude heuristic: treat .tgz/.tar as big if no content-length
    if (/\.(?:tgz|tar|zip)(?:\?|$)/i.test(u)) return 2_000_000 // ~2MB default
    return 10_000 // ~10KB default
  }

  private drain (pool: Pool) {
    const cap = this.limits[pool]
    while (this.active[pool] < cap && this.queues[pool].length > 0) {
      const next = this.queues[pool].shift()!
      this.active[pool]++
      next()
    }
  }

  private waitForSlot (pool: Pool): Promise<void> {
    const cap = this.effectiveCap()
    this.limits[pool] = Math.min(this.limits[pool], cap) // respect cap
    if (this.active[pool] < this.limits[pool]) {
      this.active[pool]++
      return Promise.resolve()
    }
    return new Promise<void>(resolve => {
      this.queues[pool].push(resolve)
      // Optional: immediate attempt to drain in case limit just grew
      this.drain(pool)
    })
  }

  private release (pool: Pool) {
    this.active[pool] = Math.max(0, this.active[pool] - 1)
    this.drain(pool)
  }

  private onFinish (pool: Pool, s: FinishSignal) {
    if (pool === 'meta') {
      this.metaLatSamples.push(s.durationMs)
      if (this.metaLatSamples.length > 100) this.metaLatSamples.shift()
      this.p95MetaLatencyMs = this.percentile(this.metaLatSamples, 0.95) ?? this.p95MetaLatencyMs
    } else {
      this.bytesWindow += s.bytes
    }
    if (s.error) this.failures++
    else this.successes++
  }

  private percentile (arr: number[], p: number): number | undefined {
    if (arr.length === 0) return undefined
    const a = [...arr].sort((x, y) => x - y)
    const idx = Math.min(a.length - 1, Math.max(0, Math.floor(p * a.length)))
    return a[idx]
  }

  private effectiveCap (): number {
    const fd = Math.max(8, Math.floor(this.fdCap / 4)) // safety margin for other fds
    return Math.min(this.hardCap, fd, this.cpuCap)
  }

  private tick () {
    const cap = this.effectiveCap()
    // CPU pressure (simple proxy using 1m load / cores)
    const [l1] = os.loadavg()
    const cores = Math.max(1, os.cpus().length)
    const cpuBusy = Math.min(1, l1 / cores) // * 0.7)

    // for logging
    const prevTar = this.limits.tar
    const prevMeta = this.limits.meta

    const bytesDelta = Math.max(0, this.bytesWindow - this.lastBytesWindow)

    // Track rolling average for better improvement detection
    this.rollingBytesWindow.push(bytesDelta)
    if (this.rollingBytesWindow.length > 5) this.rollingBytesWindow.shift()

    const avgBytes = this.rollingBytesWindow.reduce((a, b) => a + b, 0) / this.rollingBytesWindow.length
    const prevAvg = this.rollingBytesWindow.length > 1
      ? this.rollingBytesWindow.slice(0, -1).reduce((a, b) => a + b, 0) / (this.rollingBytesWindow.length - 1)
      : 0

    const improved = avgBytes > prevAvg * 1.05
    const errorRate = (this.failures) / Math.max(1, this.failures + this.successes)
    const metaTooSlow = this.p95MetaLatencyMs > 400 // arbitrary comfort threshold

    const backpressure =
      errorRate > 0.02 ||
      cpuBusy > 0.80 ||
      metaTooSlow

    // Adjust TAR pool
    if (backpressure) {
      this.limits.tar = Math.max(4, Math.floor(this.limits.tar * 0.7))
    } else if (improved && this.limits.tar < cap) {
      this.limits.tar = Math.min(cap, this.limits.tar + 2)
    } else if (!improved && this.limits.tar > 8) {
      // small decay to avoid sticking too high when no improvement
      this.limits.tar = Math.max(8, this.limits.tar - 1)
    }

    // Adjust META pool (keep small; react mainly to latency)
    if (metaTooSlow || errorRate > 0.02) {
      this.limits.meta = Math.max(2, Math.floor(this.limits.meta * 0.7))
    } else if (this.limits.meta < Math.min(6, cap)) {
      this.limits.meta += 1
    }

    // --- new logging section ---
    if ((prevTar !== this.limits.tar || prevMeta !== this.limits.meta) && process.env.PNPM_DEBUG_ADAPTIVE === 'true') {
      console.log(
        `[AdaptiveLimiter] updated concurrency → tar: ${this.limits.tar}, meta: ${this.limits.meta}, ` +
        `CPU: ${(cpuBusy * 100).toFixed(0)}%, errRate: ${(errorRate * 100).toFixed(1)}%`
      )
    }

    this.drain('tar')
    this.drain('meta')

    // reset window (keep rolling window for better adaptation)
    this.lastBytesWindow = this.bytesWindow
    this.bytesWindow = 0
    // Don't reset success/failure counts every tick - use exponential decay instead
    this.successes = Math.floor(this.successes * 0.9)
    this.failures = Math.floor(this.failures * 0.9)
  }
}

// Singleton limiter for this module (you can expose a setter if needed)
const netLimiter = new AdaptiveLimiter()

/**
 * ---------------------------
 * fetch with retries + limiter
 * ---------------------------
 */
export async function fetch (url: RequestInfo, opts: RequestInit = {}): Promise<Response> {
  const retryOpts = opts.retry ?? {}
  const maxRetries = retryOpts.retries ?? 2

  const op = operation({
    factor: retryOpts.factor ?? 10,
    maxTimeout: retryOpts.maxTimeout ?? 60000,
    minTimeout: retryOpts.minTimeout ?? 10000,
    randomize: false,
    retries: maxRetries,
  })

  // Decide pool: tar vs meta
  const urlStr = typeof url === 'string'
    ? url
    : (url as URL)?.href ?? (url as Request)?.url ?? String(url)

  // heuristic: tarballs are usually .tgz/.tar/.zip or content downloads
  const pool: Pool = /\.(?:tgz|tar|zip)(?:\?|$)/i.test(urlStr) ? 'tar' : 'meta'

  try {
    return await new Promise((resolve, reject) => {
      op.attempt(async (attempt) => {
        try {
          // Run the actual request under the adaptive limiter
          const res = await netLimiter.runFetch(pool, urlStr, async () => {
            // keep original args; node-fetch wants the Request or URL
            return nodeFetch(url as any, opts) // eslint-disable-line
          })

          // A retry on 409 sometimes helps when making requests to the Bit registry.
          if ((res.status >= 500 && res.status < 600) || [408, 409, 420, 429].includes(res.status)) {
            throw new ResponseError(res)
          } else {
            resolve(res)
          }
        } catch (error: unknown) {
          assert(util.types.isNativeError(error))
          if (
            'code' in error &&
            typeof error.code === 'string' &&
            NO_RETRY_ERROR_CODES.has(error.code)
          ) {
            throw error
          }
          const timeout = op.retry(error)
          if (timeout === false) {
            reject(op.mainError())
            return
          }
          requestRetryLogger.debug({
            attempt,
            error,
            maxRetries,
            method: opts.method ?? 'GET',
            timeout,
            url: urlStr,
            // // add visibility into live concurrency
            // concurrency: {
            // pool,
            // limit: (netLimiter as any).getConcurrency?.(pool) ?? undefined,
            // active: (netLimiter as any).activeCount?.(pool) ?? undefined,
            // },
          })
        }
      })
    })
  } catch (err) {
    if (err instanceof ResponseError) return err.res
    throw err
  }
}

const NO_RETRY_ERROR_CODES = new Set([
  'SELF_SIGNED_CERT_IN_CHAIN',
  'ERR_OSSL_PEM_NO_START_LINE',
])

export class ResponseError extends Error {
  public res: Response
  public code: number
  public status: number
  public statusCode: number
  public url: string
  constructor (res: Response) {
    super(res.statusText)
    if (Error.captureStackTrace) Error.captureStackTrace(this, ResponseError)
    this.name = this.constructor.name
    this.res = res
    this.code = this.status = this.statusCode = res.status
    this.url = res.url
  }
}
