import { bypassesCache, isShareable, matchesMetadata, metadataVariant } from './cache-policy.mjs'

// Absolute expiry also bounds fills that finish after a mutation's purge.
const RETENTION_MS = 300_000
const EXPIRES = 'x-pnpr-cdn-expires'
const CODING = 'x-pnpr-cdn-coding'

/** Cache anonymous npm reads; pnpr purges their tags when hosted data changes. */
export async function fetchWithCdnCache (request, { cache, cacheTag, fetchFromRegistry, waitUntil, now = Date.now }) {
  const route = cacheRoute(request)
  if (!route || bypassesCache(request) || request.headers.has('authorization') || request.headers.has('cookie')) {
    const response = await fetchFromRegistry(request)
    return new globalThis.URL(request.url).pathname.startsWith('/npm/') ? downstream(response) : response
  }
  const key = new globalThis.URL(request.url)
  key.searchParams.set('__pnpr_cdn', `v1-${route.variant?.name ?? 'tar'}`)
  key.searchParams.set('__pnpr_encoding', request.headers.get('accept-encoding') ?? 'absent')
  const cacheRequest = new globalThis.Request(key)
  const cached = await tryCache(() => cache.match(cacheRequest))
  if (cached) {
    const expires = Number(cached.headers.get(EXPIRES))
    if (Number.isFinite(expires) && expires > now()) return restore(cached)
    await cached.body?.cancel()
  }
  const started = now()
  const response = await fetchFromRegistry(request)
  if (!canStore(response, route)) return downstream(response)
  const expires = started + cacheLifetime(response)
  const tags = `${cacheTag},${cacheTag}-${await packageDigest(route.name)}`
  if (now() < expires) {
    waitUntil(tryCache(() => store(cache, cacheRequest, response.clone(), { expires, tags, now })))
  }
  return downstream(response)
}

function cacheLifetime (response) {
  const directives = (response.headers.get('cache-control') ?? '').toLowerCase().split(',').map(value => value.trim())
  const lifetimes = directives.filter(value => value.startsWith('max-age=') || value.startsWith('s-maxage='))
    .map(value => deltaSeconds(value.slice(value.indexOf('=') + 1)))
  const age = deltaSeconds(response.headers.get('age') ?? '0')
  if (!Number.isFinite(age) || age < 0 || lifetimes.some(value => !Number.isFinite(value) || value < 0)) return 0
  return Math.max(0, Math.min(RETENTION_MS, ...lifetimes) - age)
}

function deltaSeconds (value) {
  return /^(?:\d+|"\d+")$/.test(value) ? Number(value.replaceAll('"', '')) * 1000 : Number.NaN
}

function canStore (response, route) {
  const directives = (response.headers.get('cache-control') ?? '').toLowerCase().split(',').map(value => value.trim())
  return response.status === 200 && directives.includes('public') && isShareable(response) &&
    (!route.variant || matchesMetadata(response, route.variant))
}

async function store (cache, key, response, { expires, tags, now }) {
  const headers = new globalThis.Headers(response.headers)
  headers.delete(CODING)
  const coding = headers.get('content-encoding')
  if (coding) headers.set(CODING, coding)
  headers.delete('content-encoding')
  headers.set(EXPIRES, String(expires))
  headers.set('cache-tag', tags)
  headers.set('cache-control', `public, max-age=${Math.max(1, Math.ceil((expires - now()) / 1000))}`)
  await cache.put(key, new globalThis.Response(response.body, { headers }))
}

function restore (response) {
  const headers = new globalThis.Headers(response.headers)
  const coding = headers.get(CODING)
  if (coding) headers.set('content-encoding', coding)
  return downstream(new globalThis.Response(response.body, { headers, encodeBody: 'manual' }))
}

function downstream (response) {
  const headers = new globalThis.Headers(response.headers)
  for (const name of [CODING, EXPIRES, 'cache-tag', 'age']) headers.delete(name)
  // Caches outside this Worker cannot participate in tag invalidation.
  headers.set('cache-control', 'no-store')
  return new globalThis.Response(response.body, { status: response.status, statusText: response.statusText, headers, encodeBody: 'manual' })
}

function cacheRoute (request) {
  const pathname = decodeURIComponent(new globalThis.URL(request.url).pathname)
  if (!pathname.startsWith('/npm/')) return undefined
  let rest = pathname.slice(5)
  if (rest.startsWith('~')) {
    const slash = rest.indexOf('/')
    if (slash < 0) return undefined
    rest = rest.slice(slash + 1)
  }
  const parts = rest.split('/')
  const nameParts = rest.startsWith('@') ? 2 : 1
  if (parts.length < nameParts || parts.some(part => part === '' || part === '.' || part === '..')) return undefined
  let name = parts.slice(0, nameParts).join('/')
  if (name.startsWith('-')) return undefined
  const version = name.indexOf('@', 1)
  if (version > 0 && parts.length === nameParts) return { name: name.slice(0, version) }
  if (parts.length === nameParts + 2 && parts[nameParts] === '-' && parts.at(-1).endsWith('.tgz')) return { name }
  if (parts.length !== nameParts) return undefined
  const variant = metadataVariant(request.headers)
  return variant && { name, variant }
}

async function packageDigest (name) {
  const bytes = await globalThis.crypto.subtle.digest('SHA-256', new globalThis.TextEncoder().encode(name))
  return [...new Uint8Array(bytes)].map(byte => byte.toString(16).padStart(2, '0')).join('')
}

async function tryCache (operation) {
  try {
    return await operation()
  } catch (error) {
    globalThis.console.error('CDN cache operation failed', error)
  }
}
