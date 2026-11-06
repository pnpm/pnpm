const INSTALL_V1 = 'application/vnd.npm.install-v1+json'
const INSTALL_V2 = 'application/vnd.pnpr.install-v2+json'

/** Requests whose response must come directly from the registry. */
export function bypassesCache (request) {
  return request.method !== 'GET' || request.headers.has('origin') ||
    new globalThis.URL(request.url).search !== '' ||
    hasCacheDirective(request.headers, ['no-cache', 'no-store'])
}

/** Select a full or abbreviated slot, bypassing install-v2 offers. */
export function metadataVariant (accept) {
  if (accept?.toLowerCase().includes(INSTALL_V2)) return undefined
  return accept?.includes(INSTALL_V1)
    ? { name: 'abbreviated', contentType: INSTALL_V1 }
    : { name: 'full', contentType: 'application/json' }
}

/** The query distinguishes representations even in caches that ignore fragments. */
export function metadataKey (origin, base, name, variant) {
  const key = new globalThis.URL(`${base}${name}`, origin)
  key.searchParams.set('__pnpr_metadata', `v2-${variant.name}`)
  return key.href
}

export function matchesMetadata (response, variant) {
  return response.headers.get('content-type')?.split(';', 1)[0].trim().toLowerCase() === variant.contentType
}

/** Caller-dependent and revalidation-only responses cannot populate a shared cache. */
export function isShareable (response) {
  const vary = (response.headers.get('vary') ?? '').toLowerCase().split(',').map(value => value.trim())
  return !hasCacheDirective(response.headers, ['private', 'no-store', 'no-cache']) &&
    !vary.some(value => value === '*' || value === 'authorization' || value === 'origin')
}

function hasCacheDirective (headers, directives) {
  return (headers.get('cache-control') ?? '').split(',').some(part =>
    directives.includes(part.trim().split('=', 1)[0].trim().toLowerCase()))
}
