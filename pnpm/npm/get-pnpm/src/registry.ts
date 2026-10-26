import { createHash } from 'node:crypto'
import { createWriteStream } from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import { pipeline } from 'node:stream/promises'
import tls from 'node:tls'

import type { Packument } from './resolveVersion.js'
import type { PackageSignature } from './verifySignature.js'

/** Request headers, for a registry that needs credentials. */
export type RequestHeaders = Record<string, string>

export interface VersionMeta {
  dist: {
    tarball: string
    integrity?: string
    signatures?: PackageSignature[]
  }
}

const ABBREVIATED_PACKUMENT = 'application/vnd.npm.install-v1+json'

export const DEFAULT_REGISTRY = 'https://registry.npmjs.org/'

/** Whether credentials may be sent to `url`: over HTTPS, or to the local machine. */
export function isUrlSecureForCredentials (url: URL): boolean {
  return url.protocol === 'https:' || (url.protocol === 'http:' && isLoopback(url.hostname))
}

function isLoopback (hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' ||
    (net.isIPv4(hostname) && hostname.startsWith('127.'))
}

/**
 * A registry URL that can be used as a base for a relative path.
 *
 * `new URL('pkg', 'https://mirror.example.com/npm')` drops the last segment,
 * so a registry served from a subpath needs its trailing slash to survive.
 */
export function normalizeRegistry (registry: string): string {
  return registry.endsWith('/') ? registry : `${registry}/`
}

const PROXY_VARS = ['https_proxy', 'HTTPS_PROXY', 'http_proxy', 'HTTP_PROXY'] as const

/** The variables Node.js reads a proxy from, in the case they are spelled in. */
export type ProxyEnv = Partial<Record<typeof PROXY_VARS[number] | 'NO_PROXY' | 'no_proxy', string>>

// `http.setGlobalProxyFromEnv` arrived in Node 24.14; the types this package
// builds against predate it.
type ProxyCapableHttp = typeof http & {
  setGlobalProxyFromEnv?: (proxyEnv?: NodeJS.ProcessEnv) => () => void
}

/**
 * Routes every request made until the returned function is called through the
 * proxy `proxyEnv` names in `HTTPS_PROXY`/`HTTP_PROXY`, honouring `NO_PROXY`.
 * A no-op when it names none.
 *
 * `fetch` reads those variables only when Node was started with
 * `NODE_USE_ENV_PROXY=1`, which nothing that embeds this package controls:
 * Corepack starts the process that runs pnpm's launcher. On a network that
 * allows no other route out, the download then fails with a bare
 * `fetch failed`, while everything before it went through the proxy fine,
 * Corepack's own download of the `pnpm` package included.
 *
 * Node before 24.14 has no way to apply the proxy after startup, so the
 * request goes direct there, as it always has.
 *
 * The setting is process-global, so overlapping downloads share one
 * activation: the first call applies the proxy and the last of the returned
 * functions to run puts the agents back, never one in the middle. The proxy of
 * that first call is the one they share.
 */
export function useProxyFromEnv (proxyEnv: ProxyEnv = process.env): () => void {
  if (!PROXY_VARS.some((name) => proxyEnv[name])) return () => {}
  const { setGlobalProxyFromEnv } = http as ProxyCapableHttp
  if (setGlobalProxyFromEnv == null) return () => {}
  // Counted only once the activation took: a call that threw on a malformed
  // proxy URL holds nothing to release, and a retry after a fix has to activate.
  if (proxyUsers === 0) restoreProxy = setGlobalProxyFromEnv(proxyEnv)
  proxyUsers++
  let released = false
  return () => {
    if (released) return
    released = true
    if (--proxyUsers === 0) restoreProxy()
  }
}

let proxyUsers = 0
let restoreProxy: () => void = () => {}

/**
 * Adds `ca`, PEM certificates, to the CAs every TLS connection trusts until
 * the returned function is called. Node.js's own CAs stay trusted, so a
 * download host the registry names stays reachable.
 *
 * `fetch` takes no CA of its own, so this is process-global, like the proxy:
 * overlapping calls each add theirs, and the defaults come back once the last
 * returned function has run. A connection or TLS session opened while the CAs
 * were trusted can still be reused after that.
 *
 * Node.js before 22.19 and 24.5 cannot add a CA after startup, so there it only
 * says to use `NODE_EXTRA_CA_CERTS`.
 */
export function useCertificateAuthorities (ca: readonly string[] = []): () => void {
  if (ca.length === 0) return () => {}
  if (typeof tls.setDefaultCACertificates !== 'function') {
    console.warn(`Node.js ${process.version} cannot trust the CA certificates of the npm config after startup. Set NODE_EXTRA_CA_CERTS to a file holding them instead.`)
    return () => {}
  }
  const added = [...ca]
  const builtIn = defaultCertificates ?? tls.getCACertificates('default')
  try {
    tls.setDefaultCACertificates([...builtIn, ...[...addedCertificates, added].flat()])
  } catch (err) {
    throw new Error(`Could not trust the CA certificates of the npm config: ${(err as Error).message}`, { cause: err })
  }
  defaultCertificates = builtIn
  addedCertificates.add(added)
  return () => {
    if (!addedCertificates.delete(added)) return
    tls.setDefaultCACertificates([...builtIn, ...[...addedCertificates].flat()])
    if (addedCertificates.size === 0) defaultCertificates = undefined
  }
}

/** What `tls` trusted before the first active {@link useCertificateAuthorities} call. */
let defaultCertificates: string[] | undefined
const addedCertificates = new Set<string[]>()

// Node's fetch applies no timeout of its own: a registry that accepts the
// connection and then stalls would hang the install with no output. Metadata is
// small and quick; the tarball budget has to survive a slow connection carrying
// a ~150 MB download.
const METADATA_TIMEOUT_MS = 30_000
const TARBALL_TIMEOUT_MS = 15 * 60_000

// The tarball budget is too long to wait out a connection that stopped sending
// partway through. Going this long without a byte abandons the attempt so it
// can be retried, while a slow download that keeps making progress is left be.
const TARBALL_STALL_TIMEOUT_MS = 30_000

// A reset connection or a 502 from a load balancer in front of the registry is
// transient, and should not fail the whole install. `fetch` retries none of
// this on its own, so it is done here.
const MAX_ATTEMPTS = 3
const RETRY_DELAY_MS = 500

export async function fetchPackument (
  registry: string,
  pkgName: string,
  headers?: RequestHeaders,
  headersPath?: string
): Promise<Packument> {
  return fetchJson<Packument>(
    new URL(pkgName, registry),
    ABBREVIATED_PACKUMENT,
    { registry, headers, headersPath }
  )
}

export async function fetchVersionMeta (
  registry: string,
  pkgName: string,
  version: string,
  headers?: RequestHeaders,
  headersPath?: string
): Promise<VersionMeta> {
  return fetchJson<VersionMeta>(
    new URL(`${pkgName}/${version}`, registry),
    'application/json',
    { registry, headers, headersPath }
  )
}

/**
 * The registry signatures of `pkgName@version`.
 *
 * Some registry proxies, JFrog Artifactory among them, strip `dist.signatures`
 * from the version document but keep them in the package document, so that is
 * where they are looked up when `meta` carries none. Only the signatures are
 * taken from it: they cover `<name>@<version>:<integrity>`, so they still have
 * to verify against the checksum in `meta`, which the tarball is checked
 * against.
 */
export async function fetchSignatures (
  meta: VersionMeta,
  opts: { registry: string, pkgName: string, version: string, headers?: RequestHeaders, headersPath?: string }
): Promise<PackageSignature[] | undefined> {
  if (meta.dist.signatures?.length) return meta.dist.signatures
  // Escaped the way npm requests a scoped package, which every registry serves.
  const packument = await fetchJson<SignedPackument>(
    new URL(opts.pkgName.replaceAll('/', '%2f'), opts.registry),
    ABBREVIATED_PACKUMENT,
    { registry: opts.registry, headers: opts.headers, headersPath: opts.headersPath }
  )
  return packument.versions?.[opts.version]?.dist?.signatures
}

interface SignedPackument {
  versions?: Record<string, { dist?: { signatures?: PackageSignature[] } } | undefined>
}

/**
 * Streams `meta.dist.tarball` to `dest`, verifying the checksum the registry
 * published for it. A mismatch removes nothing — the caller discards the whole
 * temporary directory.
 *
 * `registry` re-hosts a tarball URL that points at npm onto that registry, so a
 * mirror that answered the metadata request serves the download too; `headers`
 * travel only to the registry's own origin, never to a download host it names.
 */
export async function downloadTarball (meta: VersionMeta, dest: string, opts: TarballOptions = {}): Promise<void> {
  const url = tarballUrl(meta, opts.registry)
  const [algorithm, expected] = checksum(meta)
  // The whole download is retried, not just the request: a connection reset
  // halfway through a ~150 MB body is the likeliest blip of all. Each attempt
  // rewrites `dest` from the start.
  const actual = await withRetries(() => downloadAttempt(url, dest, algorithm, opts))
  if (actual !== expected) {
    throw new Error(`The download from ${url.href} does not match the checksum the npm registry published for it. Refusing to install.`)
  }
}

/** One attempt of `downloadTarball`. @returns the digest of what was written. */
async function downloadAttempt (url: URL, dest: string, algorithm: string, opts: TarballOptions): Promise<string> {
  const stall = stallWatchdog(opts.stallTimeoutMs ?? TARBALL_STALL_TIMEOUT_MS)
  try {
    const response = await request(url, undefined, TARBALL_TIMEOUT_MS, opts, stall.signal)
    const hash = createHash(algorithm)
    await pipeline(
      readBody(response, url, TARBALL_TIMEOUT_MS),
      async function * (body: AsyncIterable<Uint8Array>) {
        for await (const chunk of body) {
          stall.reset()
          hash.update(chunk)
          yield chunk
        }
      },
      createWriteStream(dest)
    )
    return hash.digest('base64')
  } finally {
    stall.clear()
  }
}

interface ScopedHeadersOptions {
  /** Registry the credentials belong to. */
  registry?: string
  /** Credentials for `registry`, withheld from any other origin. */
  headers?: RequestHeaders
  /** Optional npmrc path scope for the credentials. */
  headersPath?: string
}

export interface TarballOptions extends ScopedHeadersOptions {
  /** Registry the metadata came from, to re-host an npm tarball URL onto. */
  registry?: string
  /** How long the download may go without receiving data before it is retried. */
  stallTimeoutMs?: number
}

/**
 * Where to download `meta`'s tarball from.
 *
 * Registries that proxy npm hand back npm's own URL. Following it would leave
 * the mirror the metadata came from — for an air-gapped one, it would not
 * resolve at all — so the path is re-hosted onto `registry`. Matched by origin,
 * so a host that merely starts with npm's is left alone.
 */
export function tarballUrl (meta: VersionMeta, registry?: string): URL {
  const url = new URL(meta.dist.tarball)
  if (registry == null || url.origin !== new URL(DEFAULT_REGISTRY).origin) return url
  return new URL(`${url.pathname.replace(/^\//, '')}${url.search}`, normalizeRegistry(registry))
}

function headersFor (url: URL, opts: ScopedHeadersOptions): RequestHeaders | undefined {
  if (opts.headers == null || opts.registry == null) return undefined
  const registry = new URL(normalizeRegistry(opts.registry))
  if (url.origin !== registry.origin) return undefined
  if (opts.headersPath == null) return opts.headers
  const scope = opts.headersPath.endsWith('/') ? opts.headersPath : `${opts.headersPath}/`
  return url.pathname.startsWith(scope) ? opts.headers : undefined
}

/**
 * The algorithm and digest to check a tarball against.
 *
 * `integrity` is an SRI string, which may hold several space-separated entries;
 * the first is used. Only the digest that the registry signature covers is
 * accepted — `shasum` is SHA-1, and a package without `integrity` has already
 * been refused before any download starts.
 */
function checksum (meta: VersionMeta): [algorithm: string, expected: string] {
  const entry = meta.dist.integrity?.trim().split(/\s+/)[0]
  if (!entry) {
    throw new Error(`The registry published no checksum for ${meta.dist.tarball}, so it cannot be verified.`)
  }
  const separator = entry.indexOf('-')
  if (separator === -1) {
    throw new Error(`The registry published an unreadable checksum for ${meta.dist.tarball}: ${entry}`)
  }
  return [entry.slice(0, separator), entry.slice(separator + 1)]
}

async function fetchJson<Result> (url: URL, accept: string, opts: ScopedHeadersOptions = {}): Promise<Result> {
  return withRetries(async () => {
    const response = await request(url, accept, METADATA_TIMEOUT_MS, opts)
    const chunks: Uint8Array[] = []
    for await (const chunk of readBody(response, url, METADATA_TIMEOUT_MS)) chunks.push(chunk)
    // Malformed JSON is what the server sent, and would be sent again, so a
    // parse failure is not retried.
    return JSON.parse(Buffer.concat(chunks).toString('utf8')) as Result
  })
}

/** A request failure, marked so `withRetries` knows whether retrying it can help. */
class RequestFailure extends Error {
  readonly retryable: boolean

  constructor (message: string, opts: { cause?: unknown, retryable: boolean }) {
    super(message, { cause: opts.cause })
    this.retryable = opts.retryable
  }
}

async function withRetries<Result> (attempt: () => Promise<Result>): Promise<Result> {
  for (let attemptNo = 1; ; attemptNo++) {
    try {
      // eslint-disable-next-line no-await-in-loop -- each attempt waits for the previous one to fail
      return await attempt()
    } catch (err) {
      if (attemptNo >= MAX_ATTEMPTS || !(err instanceof RequestFailure) || !err.retryable) throw err
      // eslint-disable-next-line no-await-in-loop -- the delay is what spaces the attempts out
      await delay(RETRY_DELAY_MS * attemptNo)
    }
  }
}

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308])
const MAX_REDIRECTS = 20

/**
 * Follows redirects by hand, so each hop gets only the credentials
 * `headersFor` allows it: `fetch` would carry them to any path on the same
 * origin, outside the scope the token was configured for.
 */
async function request (
  url: URL,
  accept: string | undefined,
  timeoutMs: number,
  opts: ScopedHeadersOptions = {},
  stallSignal?: AbortSignal
): Promise<Response> {
  const timeout = AbortSignal.timeout(timeoutMs)
  const signal = stallSignal == null ? timeout : AbortSignal.any([timeout, stallSignal])
  let currentUrl = url

  for (let redirects = 0; ; redirects++) {
    // eslint-disable-next-line no-await-in-loop -- each hop is known only once the previous one answers
    const response = await fetchHop(currentUrl, { accept, signal, timeoutMs, opts })
    if (!REDIRECT_STATUSES.has(response.status) && response.ok) return response

    // Left unread, the body would hold the connection until it is collected.
    // eslint-disable-next-line no-await-in-loop -- the connection is released before the next hop opens one
    await response.body?.cancel().catch(() => {})
    const target = redirectTarget(response, currentUrl)
    if (redirects >= MAX_REDIRECTS) {
      throw new RequestFailure(`Could not download ${url.href}: too many redirects`, { retryable: false })
    }
    currentUrl = target
  }
}

async function fetchHop (
  url: URL,
  { accept, signal, timeoutMs, opts }: { accept: string | undefined, signal: AbortSignal, timeoutMs: number, opts: ScopedHeadersOptions }
): Promise<Response> {
  try {
    return await fetch(url, {
      redirect: 'manual',
      signal,
      headers: { ...headersFor(url, opts), ...(accept ? { accept } : {}) },
    })
  } catch (err) {
    throw new RequestFailure(`Could not reach ${url.href}: ${reasonFor(err, timeoutMs)}`, { cause: err, retryable: isRetryableError(err, timeoutMs) })
  }
}

/** Where the redirect `response` points; throws for a response that is not one. */
function redirectTarget (response: Response, url: URL): URL {
  if (!REDIRECT_STATUSES.has(response.status)) {
    throw new RequestFailure(`Could not download ${url.href}: ${response.status} ${response.statusText}`, { retryable: isRetryableStatus(response.status) })
  }
  const location = response.headers.get('location')
  if (location == null) {
    throw new RequestFailure(`Could not download ${url.href}: ${response.status} redirect without a Location header`, { retryable: false })
  }
  return new URL(location, url)
}

/**
 * `response`'s body, with a connection lost partway through, or a body that
 * never came, marked as worth retrying. A 200 with nothing in it is a proxy or
 * CDN hiccup, not an answer.
 */
async function * readBody (response: Response, url: URL, timeoutMs: number): AsyncIterable<Uint8Array> {
  let received = 0
  if (response.body != null) {
    try {
      for await (const chunk of response.body as unknown as AsyncIterable<Uint8Array>) {
        received += chunk.byteLength
        yield chunk
      }
    } catch (err) {
      throw interrupted(url, err, timeoutMs)
    }
  }
  if (received === 0) {
    throw new RequestFailure(`Empty response from ${url.href}`, { retryable: true })
  }
}

/**
 * A signal that aborts once `timeoutMs` passes without a call to `reset`.
 *
 * It aborts with a plain `Error` rather than a `TimeoutError`, so the failure
 * reads as an interrupted connection and is retried like one.
 */
function stallWatchdog (timeoutMs: number): { signal: AbortSignal, reset: () => void, clear: () => void } {
  const controller = new AbortController()
  let timer: NodeJS.Timeout | undefined
  const clear = (): void => {
    clearTimeout(timer)
  }
  const reset = (): void => {
    clear()
    timer = setTimeout(() => {
      controller.abort(new Error(`no data received for ${Math.round(timeoutMs / 1000)}s`))
    }, timeoutMs)
  }
  reset()
  return { signal: controller.signal, reset, clear }
}

function interrupted (url: URL, err: unknown, timeoutMs: number): RequestFailure {
  return new RequestFailure(`The download from ${url.href} was interrupted: ${reasonFor(err, timeoutMs)}`, { cause: err, retryable: isRetryableError(err, timeoutMs) })
}

function reasonFor (err: unknown, timeoutMs: number): string {
  return (err as Error).name === 'TimeoutError'
    ? `timed out after ${Math.round(timeoutMs / 1000)}s`
    : (err as Error).message
}

/**
 * A connection that could not be made, or was reset, is exactly the kind of
 * blip retrying is for. So is a metadata request that stalled. A tarball that
 * ran out its whole budget is not: that budget is already sized for a slow
 * connection, and each retry would hang the install for as long again.
 */
function isRetryableError (err: unknown, timeoutMs: number): boolean {
  return (err as Error).name !== 'TimeoutError' || timeoutMs < TARBALL_TIMEOUT_MS
}

/**
 * A 4xx means the request itself is wrong (a missing package, a bad
 * credential) — retrying would just ask the same wrong question again. The
 * exceptions are a request timeout and rate limiting, which say "not now"
 * rather than "no". A 5xx is the server's problem, and often a transient one.
 */
function isRetryableStatus (status: number): boolean {
  return status === 408 || status === 429 || status >= 500
}

async function delay (ms: number): Promise<void> {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}
