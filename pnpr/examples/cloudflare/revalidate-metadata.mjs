import { bypassesCache, isShareable, matchesMetadata, metadataKey, metadataVariant } from './cache-policy.mjs'

const STORED_HEADERS = [
  ['content-encoding', 'x-pnpr-cache-encoding'],
  ['cache-control', 'x-pnpr-cache-control'],
  ['etag', 'x-pnpr-cache-etag'],
]

/** Cache metadata bodies, but let the registry decide freshness and access on every read. */
export async function fetchMetadata (request, { cache, base, name, fetchFromRegistry, waitUntil }) {
  const variant = metadataVariant(request.headers)
  if (bypassesCache(request) || !variant) return fetchFromRegistry(request)

  const origin = new globalThis.URL(request.url).origin
  const key = new globalThis.Request(metadataKey(origin, base, name, variant))
  const stored = await tryCache(() => cache.match(key))
  const cached = stored && restore(stored)
  const headers = new globalThis.Headers(request.headers)
  if (cached) headers.set('if-none-match', cached.headers.get('etag'))
  let response = await fetchFromRegistry(new globalThis.Request(request, { headers }))
  let shouldStore = true

  if (response.status === 304 && cached) {
    const merged = new globalThis.Headers(cached.headers)
    for (const [name, value] of response.headers) {
      if (name !== 'content-length') merged.set(name, value)
    }
    shouldStore = !sameHeaders(cached.headers, merged)
    response = new globalThis.Response(cached.body, { headers: merged, encodeBody: 'manual' })
    if (!isShareable(response) || !matchesMetadata(response, variant)) {
      await response.body?.cancel()
      response = await fetchFromRegistry(request)
      shouldStore = true
    }
  } else {
    await cached?.body?.cancel()
  }

  if (response.status === 200 && response.headers.has('etag') &&
      isShareable(response) && matchesMetadata(response, variant)) {
    if (shouldStore) waitUntil(tryCache(() => cache.put(key, prepare(response.clone()))))
  } else {
    waitUntil(tryCache(() => cache.delete(key)))
  }
  return response
}

async function tryCache (operation) {
  try {
    return await operation()
  } catch (error) {
    globalThis.console.error('Metadata cache operation failed', error)
  }
}

function sameHeaders (first, second) {
  const entries = [...first]
  return entries.length === [...second].length &&
    entries.every(([name, value]) => second.get(name) === value)
}

// Keep the Cache API from decoding the bytes or evaluating their validator.
function prepare (response) {
  const headers = new globalThis.Headers(response.headers)
  for (const [header, stored] of STORED_HEADERS) {
    const value = headers.get(header)
    headers.delete(stored)
    if (value !== null) headers.set(stored, value)
    headers.delete(header)
  }
  headers.set('cache-control', 'public, max-age=86400')
  return new globalThis.Response(response.body, { headers })
}

function restore (response) {
  const headers = new globalThis.Headers(response.headers)
  for (const [header, stored] of STORED_HEADERS) {
    const value = headers.get(stored)
    headers.delete(header)
    headers.delete(stored)
    if (value !== null) headers.set(header, value)
  }
  return new globalThis.Response(response.body, { headers, encodeBody: 'manual' })
}
