/// <reference path="../../../typings/index.d.ts"/>
import { createFetchFromRegistry } from '@pnpm/fetch'
import nock = require('nock')

test('fetchFromRegistry', async () => {
  const fetchFromRegistry = createFetchFromRegistry({})
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  expect(metadata.name).toEqual('is-positive')
  expect(metadata.versions['1.0.0'].scripts).not.toBeTruthy()
})

test('fetchFromRegistry fullMetadata', async () => {
  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  expect(metadata.name).toEqual('is-positive')
  expect(metadata.versions['1.0.0'].scripts).toBeTruthy()
})

test('authorization headers are removed before redirection if the target is on a different host', async () => {
  nock('http://registry.pnpm.js.org/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive')
    .reply(302, '', { location: 'http://registry.other.org/is-positive' })
  nock('http://registry.other.org/', { badheaders: ['authorization'] })
    .get('/is-positive')
    .reply(200, { ok: true })

  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry(
    'http://registry.pnpm.js.org/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  expect(await res.json()).toStrictEqual({ ok: true })
  expect(nock.isDone()).toBeTruthy()
})

test('authorization headers are not removed before redirection if the target is on the same host', async () => {
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

  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry(
    'http://registry.pnpm.js.org/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  expect(await res.json()).toStrictEqual({ ok: true })
  expect(nock.isDone()).toBeTruthy()
})

test('switch to the correct agent for requests on redirect from http: to https:', async () => {
  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })

  // We can test this on any endpoint that redirects from http: to https:
  const { status } = await fetchFromRegistry('http://pnpm.js.org/css/main.css')

  expect(status).toEqual(200)
})
