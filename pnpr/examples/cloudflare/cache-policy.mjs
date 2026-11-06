const INSTALL_V1 = 'application/vnd.npm.install-v1+json'
const INSTALL_V2 = 'application/vnd.pnpr.install-v2+json'
const ENCODINGS = [null, '', 'identity', 'gzip', 'deflate', 'br', 'zstd', 'zstd, gzip']

/** Requests whose response must come directly from the registry. */
export function bypassesCache (request) {
  return request.method !== 'GET' ||
    ['origin', 'if-none-match', 'if-modified-since', 'if-match', 'if-unmodified-since'].some(header => request.headers.has(header)) ||
    new globalThis.URL(request.url).search !== '' ||
    hasCacheDirective(request.headers, ['no-cache', 'no-store'])
}

/** Select a cache slot from the headers sent to the registry; install-v2 bypasses it. */
export function metadataVariant (headers) {
  const accept = headers.get('accept')
  const encoding = headers.get('accept-encoding')
  if (accept?.toLowerCase().includes(INSTALL_V2) || !ENCODINGS.includes(encoding)) return undefined
  return accept?.includes(INSTALL_V1)
    ? { name: 'abbreviated', contentType: INSTALL_V1, encoding }
    : { name: 'full', contentType: 'application/json', encoding }
}

/** The query distinguishes representations even in caches that ignore fragments. */
export function metadataKey (origin, base, name, variant) {
  const key = new globalThis.URL(`${base}${name}`, origin)
  key.searchParams.set('__pnpr_metadata', `v5-${variant.name}`)
  key.searchParams.set('__pnpr_encoding', variant.encoding ?? 'absent')
  return key.href
}

/** Every metadata key for a package route, for invalidation. */
export function metadataKeys (origin, base, name) {
  return ENCODINGS.flatMap(encoding => ['application/json', INSTALL_V1].map(accept => {
    const headers = new globalThis.Headers({ accept })
    if (encoding !== null) headers.set('accept-encoding', encoding)
    const variant = metadataVariant(headers)
    return metadataKey(origin, base, name, variant)
  }))
}

export function matchesMetadata (response, variant) {
  return response.headers.get('content-type')?.split(';', 1)[0].trim().toLowerCase() === variant.contentType
}

/** Caller-dependent and revalidation-only responses cannot populate a shared cache. */
export function isShareable (response) {
  const vary = (response.headers.get('vary') ?? '').toLowerCase().split(',').map(value => value.trim())
  return !hasCacheDirective(response.headers, ['private', 'no-store', 'no-cache']) &&
    vary.every(value => value === '' || value === 'accept' || value === 'accept-encoding')
}

function hasCacheDirective (headers, directives) {
  return (headers.get('cache-control') ?? '').split(',').some(part =>
    directives.includes(part.trim().split('=', 1)[0].trim().toLowerCase()))
}
