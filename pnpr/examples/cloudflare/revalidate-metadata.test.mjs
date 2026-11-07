import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { setImmediate } from 'node:timers'
import { test } from 'node:test'
import { gzipSync } from 'node:zlib'
import { fetchMetadata } from './revalidate-metadata.mjs'

function metadata (body = 'original', headers = {}) {
  return new globalThis.Response(body, {
    headers: {
      'content-type': 'application/json',
      'cache-control': 'public, max-age=300',
      etag: '"original"',
      vary: 'Accept, Accept-Encoding',
      ...headers,
    },
  })
}

function fixture () {
  const entries = new Map()
  const cache = {
    async match (key) {
      const entry = entries.get(key.url)
      return entry && new globalThis.Response(entry.body, { headers: entry.headers })
    },
    async put (key, response) {
      entries.set(key.url, { body: await response.arrayBuffer(), headers: response.headers })
    },
    async delete (key) { return entries.delete(key.url) },
  }
  const calls = []
  let origin = () => metadata()
  return {
    entries,
    calls,
    setOrigin (callback) { origin = callback },
    read (headers = {}, path = '/npm/widget', method = 'GET', selectedCache = cache) {
      return fetchMetadata(new globalThis.Request(`https://registry.example${path}`, { headers, method }), {
        cache: selectedCache,
        base: '/npm/',
        name: 'widget',
        fetchFromRegistry: async request => {
          calls.push(request)
          return origin(request)
        },
      })
    },
  }
}

test('every warm read validates the stored body, preserving URL and credentials', async () => {
  const server = fixture()
  assert.equal(await (await server.read()).text(), 'original')
  server.setOrigin(request => {
    assert.equal(request.url, 'https://registry.example/npm/widget')
    assert.equal(request.headers.get('authorization'), 'Bearer example')
    assert.equal(request.headers.get('if-none-match'), '"original"')
    return new globalThis.Response(null, { status: 304, headers: { 'cache-control': 'public, max-age=10', 'content-length': '0' } })
  })
  const response = await server.read({ authorization: 'Bearer example' })
  assert.equal(response.status, 200)
  assert.equal(await response.text(), 'original')
  assert.equal(response.headers.get('cache-control'), 'public, max-age=10')
  assert.equal(response.headers.get('content-length'), null)
  assert.equal(server.calls.length, 2)
})

test('a completed mutation is observed in independently warmed data centers', async () => {
  const locations = [fixture(), fixture()]
  for (const server of locations) {
    await (await server.read()).body.cancel()
  }
  for (const server of locations) {
    server.setOrigin(() => metadata('changed', { etag: '"changed"' }))
    assert.equal(await (await server.read()).text(), 'changed')
    server.setOrigin(() => new globalThis.Response(null, { status: 304 }))
    assert.equal(await (await server.read()).text(), 'changed')
  }
})

for (const status of [401, 403, 404, 500, 503]) {
  test(`a registry ${status} never serves a cached success`, async () => {
    const server = fixture()
    await (await server.read()).body.cancel()
    server.setOrigin(() => new globalThis.Response('denied', { status }))
    const response = await server.read()
    assert.equal(response.status, status)
    assert.equal(await response.text(), 'denied')
    assert.equal(server.entries.size, 0)
  })
}

test('network failure propagates without a stale response', async () => {
  const server = fixture()
  await (await server.read()).body.cancel()
  server.setOrigin(() => { throw new Error('origin unavailable') })
  await assert.rejects(server.read(), /origin unavailable/)
})

for (const headers of [
  { 'cache-control': 'private, no-store' },
  { vary: 'Authorization' },
  { 'set-cookie': 'session=example' },
  { 'content-type': 'application/vnd.pnpr.install-v2+json' },
]) {
  test(`a newly restricted response evicts the shared body: ${JSON.stringify(headers)}`, async () => {
    const server = fixture()
    await (await server.read()).body.cancel()
    server.setOrigin(() => metadata('restricted', headers))
    assert.equal(await (await server.read()).text(), 'restricted')
    assert.equal(server.entries.size, 0)
  })
}

test('a restrictive 304 requires a full response and evicts the shared copy', async () => {
  const server = fixture()
  await (await server.read()).body.cancel()
  server.setOrigin(request => request.headers.has('if-none-match')
    ? new globalThis.Response(null, { status: 304, headers: { 'cache-control': 'private, no-store' } })
    : metadata('private', { 'cache-control': 'private, no-store' }))
  assert.equal(await (await server.read()).text(), 'private')
  assert.equal(server.entries.size, 0)
  assert.equal(server.calls.length, 3)
})

for (const etag of ['"original"', 'W/"original"', '*', '"other", W/"original"']) {
  test(`client validators reach the registry unchanged for its 304 handling: ${etag}`, async () => {
    const server = fixture()
    await (await server.read()).body.cancel()
    server.setOrigin(request => {
      assert.equal(request.headers.get('if-none-match'), etag)
      return new globalThis.Response(null, { status: 304, headers: { etag: '"original"' } })
    })
    const response = await server.read({ 'if-none-match': etag })
    assert.equal(response.status, 304)
    assert.equal(response.body, null)
  })
}

test('compression and validators survive storage without exposing internal headers', async () => {
  const server = fixture()
  const bytes = gzipSync('original')
  server.setOrigin(() => metadata(bytes, { 'content-encoding': 'gzip', 'content-length': String(bytes.length) }))
  await (await server.read({ 'accept-encoding': 'gzip' })).body.cancel()
  const stored = [...server.entries.values()][0]
  assert.equal(stored.headers.get('content-encoding'), null)
  assert.equal(stored.headers.get('etag'), null)
  server.setOrigin(() => new globalThis.Response(null, { status: 304 }))
  const response = await server.read({ 'accept-encoding': 'gzip' })
  assert.deepEqual(Buffer.from(await response.arrayBuffer()), bytes)
  assert.equal(response.headers.get('content-encoding'), 'gzip')
  assert.equal(response.headers.get('content-length'), String(bytes.length))
  assert.equal(response.headers.get('etag'), '"original"')
  assert.equal([...response.headers.keys()].some(name => name.startsWith('x-pnpr-cache-')), false)
})

test('full, abbreviated, and encoded representations use separate bodies', async () => {
  const server = fixture()
  server.setOrigin(request => metadata(request.headers.get('accept') ?? 'full', {
    'content-type': request.headers.get('accept') ?? 'application/json',
  }))
  for (const headers of [{}, { accept: 'application/vnd.npm.install-v1+json' }, { 'accept-encoding': 'gzip' }]) {
    await (await server.read(headers)).body.cancel()
  }
  assert.equal(server.entries.size, 3)
})

test('responses without ETags cannot populate the cache', async () => {
  const server = fixture()
  server.setOrigin(() => {
    const response = metadata()
    response.headers.delete('etag')
    return response
  })
  await (await server.read()).body.cancel()
  assert.equal(server.entries.size, 0)
})

for (const [headers, path, method] of [
  [{}, '/npm/widget?write=true', 'GET'],
  [{ 'cache-control': 'no-cache' }, '/npm/widget', 'GET'],
  [{ range: 'bytes=0-1' }, '/npm/widget', 'GET'],
  [{ origin: 'https://client.example' }, '/npm/widget', 'GET'],
  [{ accept: 'application/vnd.pnpr.install-v2+json' }, '/npm/widget', 'GET'],
  [{}, '/-/pnpr/v0/staged/example/approve', 'POST'],
  [{}, '/-/pnpr/v0/admin/rules/npm/local', 'PUT'],
]) {
  test(`bypass never touches the cache: ${method} ${path} ${JSON.stringify(headers)}`, async () => {
    const server = fixture()
    const cache = new Proxy({}, { get () { throw new Error('cache must not be accessed') } })
    const response = await server.read(headers, path, method, cache)
    assert.equal(await response.text(), 'original')
  })
}

test('an old fill arriving after a write is revalidated on the next read', async () => {
  const server = fixture()
  let finishOldRead
  server.setOrigin(() => new Promise(resolve => { finishOldRead = resolve }))
  const pending = server.read()
  await new Promise(resolve => setImmediate(resolve))
  server.setOrigin(() => metadata('new', { etag: '"new"' }))
  assert.equal(await (await server.read()).text(), 'new')
  finishOldRead(metadata())
  await (await pending).body.cancel()
  assert.equal(await (await server.read()).text(), 'new')
})
