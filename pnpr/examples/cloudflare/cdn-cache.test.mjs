import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { test } from 'node:test'
import { fetchWithCdnCache } from './cdn-cache.mjs'

function fixture () {
  const entries = new Map()
  const pending = []
  let timestamp = 1_000_000
  let calls = 0
  let status = 200
  let body = 'old'
  const cache = {
    async match (key) {
      const stored = entries.get(key.url)
      return stored && new globalThis.Response(await stored.clone().arrayBuffer(), { headers: stored.headers })
    },
    async put (key, response) { entries.set(key.url, new globalThis.Response(await response.arrayBuffer(), { headers: response.headers })) },
  }
  return {
    entries,
    get calls () { return calls },
    advance (milliseconds) { timestamp += milliseconds },
    change (value, code = 200) { body = value; status = code },
    purge (tag) {
      for (const [key, response] of entries) {
        if (response.headers.get('cache-tag').split(',').includes(tag)) entries.delete(key)
      }
    },
    async get (path = '/npm/widget', headers = {}, options = {}) {
      const response = await fetchWithCdnCache(new globalThis.Request(`https://registry.example${path}`, { headers }), {
        cache, cacheTag: 'example', waitUntil: task => pending.push(task), now: () => timestamp,
        fetchFromRegistry: async () => {
          calls++
          return new globalThis.Response(body, { status, headers: { 'cache-control': 'public, max-age=300', 'content-type': headers.accept ?? 'application/json', etag: '"example"' } })
        },
        ...options,
      })
      await Promise.all(pending.splice(0))
      return response
    },
  }
}

for (const name of ['widget', '@example/widget']) {
  test(`package tag purges aliases, representations and tarballs: ${name}`, async () => {
    const setup = fixture()
    const paths = ['', '~local/', '~main/', '~tgz/'].flatMap(base => [`/npm/${base}${name}`, `/npm/${base}${name}@1.0.0`, `/npm/${base}${name}/-/widget-1.0.0.tgz`])
    for (const path of paths) {
      for (const accept of ['application/json', 'application/vnd.npm.install-v1+json']) {
        await setup.get(path, { accept })
        assert.equal(await (await setup.get(path, { accept })).text(), 'old')
      }
    }
    assert.equal(setup.calls, 16)
    setup.change('new')
    setup.purge(`example-${createHash('sha256').update(name).digest('hex')}`)
    assert.equal(setup.entries.size, 0)
    for (const path of paths) assert.equal(await (await setup.get(path)).text(), 'new')
  })
}

test('policy purge removes cached metadata and tarballs before access is checked again', async () => {
  const setup = fixture()
  for (const path of ['/npm/widget', '/npm/widget@1.0.0']) await setup.get(path)
  setup.change('denied', 403)
  setup.purge('example')
  for (const path of ['/npm/widget', '/npm/widget@1.0.0']) assert.equal((await setup.get(path)).status, 403)
  assert.equal(setup.entries.size, 0)
})

test('cache hits avoid the origin, but failed purges cannot extend expiry', async () => {
  const setup = fixture()
  await setup.get()
  setup.change('gone', 404)
  assert.equal(await (await setup.get()).text(), 'old')
  assert.equal(setup.calls, 1)
  setup.advance(300_000)
  assert.equal((await setup.get()).status, 404)
  assert.equal(setup.calls, 2)
})

test('a late fill cannot restart the cache lifetime', async () => {
  const setup = fixture()
  await setup.get('/npm/widget', {}, { fetchFromRegistry: async () => {
    setup.advance(300_001)
    return new globalThis.Response('late', { headers: { 'cache-control': 'public', 'content-type': 'application/json' } })
  } })
  assert.equal(setup.entries.size, 0)
})

for (const headers of [{ authorization: 'Bearer token' }, { cookie: 'session=example' }, { origin: 'https://client.example' }, { 'cache-control': 'no-cache' }, { 'if-none-match': '"test"' }, { accept: 'application/vnd.pnpr.install-v2+json' }]) {
  test(`bypasses cache for ${JSON.stringify(headers)}`, async () => {
    const setup = fixture()
    await setup.get()
    setup.change('current')
    assert.equal(await (await setup.get('/npm/widget', headers)).text(), 'current')
    assert.equal(setup.calls, 2)
  })
}

for (const path of ['/npm/widget?write=true', '/npm/-/tarballs/sha512/example', '/-/pnpr/v0/registries']) {
  test(`does not cache ${path}`, async () => {
    const setup = fixture()
    await setup.get(path)
    await setup.get(path)
    assert.equal(setup.entries.size, 0)
    assert.equal(setup.calls, 2)
  })
}

for (const control of ['private', 'no-store', 'no-cache', 'max-age=300']) {
  test(`does not cache responses without explicit public permission: ${control}`, async () => {
    const setup = fixture()
    await setup.get('/npm/widget', {}, { fetchFromRegistry: async () => new globalThis.Response('secret', { headers: { 'cache-control': control, 'content-type': 'application/json' } }) })
    assert.equal(setup.entries.size, 0)
  })
}

test('compressed bytes survive storage and internal headers stay private', async () => {
  const setup = fixture()
  const bytes = new Uint8Array([1, 2, 3])
  await setup.get('/npm/widget', {}, { fetchFromRegistry: async () => new globalThis.Response(bytes, { headers: { 'cache-control': 'public', 'content-type': 'application/json', 'content-encoding': 'zstd' } }) })
  const response = await setup.get()
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), bytes)
  assert.equal(response.headers.get('content-encoding'), 'zstd')
  assert.equal(response.headers.get('cache-control'), 'no-store')
  for (const header of ['cache-tag', 'x-pnpr-cdn-expires', 'x-pnpr-cdn-coding']) assert.equal(response.headers.get(header), null)
})

for (const headers of [{ 'cache-control': 'public, max-age=0' }, { 'cache-control': 'public, max-age=300', age: '300' }, { 'cache-control': 'public', 'set-cookie': 'secret=value' }]) {
  test(`does not retain expired or cookie-bearing responses: ${JSON.stringify(headers)}`, async () => {
    const setup = fixture()
    await setup.get('/npm/widget', {}, { fetchFromRegistry: async () => new globalThis.Response('body', { headers: { ...headers, 'content-type': 'application/json' } }) })
    assert.equal(setup.entries.size, 0)
  })
}
