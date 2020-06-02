///<reference path="../../../typings/index.d.ts"/>
import createRegClient from 'fetch-from-npm-registry'
import nock = require('nock')
import test = require('tape')

test('fetchFromNpmRegistry', async t => {
  const fetchFromNpmRegistry = createRegClient({})
  const res = await fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  t.equal(metadata.name, 'is-positive')
  t.notOk(metadata.versions['1.0.0'].scripts)
  t.end()
})

test('fetchFromNpmRegistry fullMetadata', async t => {
  const fetchFromNpmRegistry = createRegClient({ fullMetadata: true })
  const res = await fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  t.equal(metadata.name, 'is-positive')
  t.ok(metadata.versions['1.0.0'].scripts)
  t.end()
})

test('authorization headers are removed before redirection if the target is on a different host', async (t) => {
  nock('http://registry.pnpm.js.org/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive')
    .reply(302, '', { location: 'http://registry.other.org/is-positive' })
  nock('http://registry.other.org/', { badheaders: ['authorization'] })
    .get('/is-positive')
    .reply(200, { ok: true })

  const fetchFromNpmRegistry = createRegClient({ fullMetadata: true })
  const res = await fetchFromNpmRegistry(
    'http://registry.pnpm.js.org/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  t.deepEqual(await res.json(), { ok: true })
  t.ok(nock.isDone())
  t.end()
})

test('authorization headers are not removed before redirection if the target is on the same host', async (t) => {
  nock('http://registry.pnpm.js.org/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive')
    .reply(302, '', { location: 'http://registry.pnpm.js.org/is-positive-new' })
  nock('http://registry.pnpm.js.org/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive-new')
    .reply(200, { ok: true })

  const fetchFromNpmRegistry = createRegClient({ fullMetadata: true })
  const res = await fetchFromNpmRegistry(
    'http://registry.pnpm.js.org/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  t.deepEqual(await res.json(), { ok: true })
  t.ok(nock.isDone())
  t.end()
})

test('switch to the correct agent for requests on redirect from http: to https:', async (t) => {
  const fetchFromNpmRegistry = createRegClient({ fullMetadata: true })

  // We can test this on any endpoint that redirects from http: to https:
  const { status } = await fetchFromNpmRegistry('http://pnpm.js.org/css/main.css')

  t.equal(status, 200)
  t.end()
})
