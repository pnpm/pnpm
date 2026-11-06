import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bypassesCache, isShareable, matchesMetadata, metadataKey, metadataKeys, metadataVariant } from './cache-policy.mjs'

const INSTALL_V1 = 'application/vnd.npm.install-v1+json'
const INSTALL_V2 = 'application/vnd.pnpr.install-v2+json'

function request (headers = {}, query = '', method = 'GET') {
  return new globalThis.Request(`https://registry.example/npm/widget${query}`, { headers, method })
}

function response (contentType = 'application/json', headers = {}) {
  return new globalThis.Response('{}', { headers: { 'content-type': contentType, ...headers } })
}

for (const accept of [INSTALL_V2, `${INSTALL_V2}; q=1.0, ${INSTALL_V1}; q=0.9`]) {
  test(`install-v2 bypasses both metadata slots: ${accept}`, () => {
    assert.equal(metadataVariant(new globalThis.Headers(accept ? { accept } : {})), undefined)
  })
}

test('full and abbreviated metadata have distinct keys after URL fragments are removed', () => {
  const full = metadataVariant(new globalThis.Headers())
  const abbreviated = metadataVariant(new globalThis.Headers({ accept: INSTALL_V1 }))
  const fullKey = new globalThis.URL(metadataKey('https://registry.example', '/npm/', '@acme/widget', full))
  const abbreviatedKey = new globalThis.URL(metadataKey('https://registry.example', '/npm/', '@acme/widget', abbreviated))
  fullKey.hash = ''
  abbreviatedKey.hash = ''
  assert.notEqual(fullKey.href, abbreviatedKey.href)
  assert.equal(fullKey.pathname, '/npm/@acme/widget')
  assert.notEqual(fullKey.search, '')
  assert.notEqual(abbreviatedKey.search, '')
  assert.notEqual(fullKey.href, 'https://registry.example/npm/@acme/widget')
  assert.notEqual(fullKey.href, metadataKey('https://registry.example', '/npm/~other/', '@acme/widget', full))
})

test('each metadata slot accepts only its response representation', () => {
  for (const accept of [null, INSTALL_V1]) {
    const variant = metadataVariant(new globalThis.Headers(accept ? { accept } : {}))
    const expected = accept ?? 'application/json'
    for (const contentType of ['application/json', INSTALL_V1, INSTALL_V2, 'text/html']) {
      assert.equal(matchesMetadata(response(contentType), variant), contentType === expected)
    }
    assert.equal(matchesMetadata(response(`${expected}; charset=utf-8`), variant), true)
    assert.equal(matchesMetadata(new globalThis.Response('{}'), variant), false)
  }
})

for (const [headers, query, method] of [
  [{}, '?write=true', 'GET'],
  [{}, '?other=value', 'GET'],
  [{ 'cache-control': 'no-cache' }, '', 'GET'],
  [{ 'cache-control': 'max-age=10, No-Cache' }, '', 'GET'],
  [{ 'cache-control': 'no-store' }, '', 'GET'],
  [{ origin: 'https://client.example' }, '', 'GET'],
  [{ 'if-none-match': '"revision"' }, '', 'GET'],
  [{ 'if-modified-since': 'Fri, 06 Nov 2026 00:00:00 GMT' }, '', 'GET'],
  [{ 'if-match': '"revision"' }, '', 'GET'],
  [{ 'if-unmodified-since': 'Fri, 06 Nov 2026 00:00:00 GMT' }, '', 'GET'],
  [{}, '', 'PUT'],
  [{}, '', 'HEAD'],
]) {
  test(`bypasses cache: ${method} ${query} ${JSON.stringify(headers)}`, () => {
    assert.equal(bypassesCache(request(headers, query, method)), true)
  })
}

test('ordinary GET requests can use the cache', () => {
  assert.equal(bypassesCache(request()), false)
  assert.equal(bypassesCache(request({ 'cache-control': 'max-age=300' })), false)
})

for (const headers of [
  { 'cache-control': 'private, max-age=300' },
  { 'cache-control': 'public, NO-STORE' },
  { 'cache-control': 'no-cache="etag"' },
  { vary: 'Accept, Authorization' },
  { vary: '*' },
  { vary: 'Accept, Origin' },
  { vary: 'Cookie' },
  { vary: 'Accept, X-Registry-View' },
]) {
  test(`does not share restricted responses: ${JSON.stringify(headers)}`, () => {
    assert.equal(isShareable(response('application/json', headers)), false)
  })
}

test('public metadata with Vary: Accept remains cacheable', () => {
  assert.equal(isShareable(response('application/json', { 'cache-control': 'public, max-age=300', vary: 'Accept' })), true)
})

test('encoding preferences have distinct metadata keys', () => {
  const encodings = [null, '', 'identity', 'gzip', 'deflate', 'br', 'zstd', 'zstd, gzip']
  const keys = encodings.map(encoding => metadataKey(
    'https://registry.example', '/npm/', 'widget',
    metadataVariant(new globalThis.Headers(encoding === null ? {} : { 'accept-encoding': encoding })),
  ))
  assert.equal(new Set(keys).size, encodings.length)
  for (let index = 0; index < keys.length; index++) {
    assert.equal(new globalThis.URL(keys[index]).searchParams.get('__pnpr_encoding'), encodings[index] ?? 'absent')
  }
})

test('metadata varying by encoding remains shareable', () => {
  assert.equal(isShareable(response('application/json', {
    'cache-control': 'public, max-age=300', vary: 'Accept, Accept-Encoding',
  })), true)
})

for (const encoding of ['gzip;q=0.5, br;q=1.0', 'unknown', 'gzip, br', 'gzip, gzip']) {
  test(`unsupported encoding preferences bypass the metadata cache: ${encoding}`, () => {
    assert.equal(metadataVariant(new globalThis.Headers(encoding === null ? {} : { 'accept-encoding': encoding })), undefined)
  })
}

test('invalidation enumerates both formats and every supported encoding', () => {
  const keys = metadataKeys('https://registry.example', '/npm/', 'widget')
  assert.equal(new Set(keys).size, 16)
  for (const encoding of [null, '', 'identity', 'gzip', 'deflate', 'br', 'zstd', 'zstd, gzip']) {
    for (const accept of ['application/json', INSTALL_V1]) {
      const headers = new globalThis.Headers({ accept })
      if (encoding !== null) headers.set('accept-encoding', encoding)
      const variant = metadataVariant(headers)
      assert.ok(keys.includes(metadataKey('https://registry.example', '/npm/', 'widget', variant)))
    }
  }
})

test('absent and empty encoding preferences have distinct keys', () => {
  const absent = metadataVariant(new globalThis.Headers())
  const empty = metadataVariant(new globalThis.Headers({ 'accept-encoding': '' }))
  assert.notEqual(
    metadataKey('https://registry.example', '/npm/', 'widget', absent),
    metadataKey('https://registry.example', '/npm/', 'widget', empty),
  )
})
