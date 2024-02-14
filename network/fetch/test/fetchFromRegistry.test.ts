/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import nock from 'nock'
import { ProxyServer } from 'https-proxy-server-express'
import fs from 'fs'

const CERTS_DIR = path.join(__dirname, '__certs__')

test('fetchFromRegistry', async () => {
  const fetchFromRegistry = createFetchFromRegistry({})
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json() as any // eslint-disable-line
  expect(metadata.name).toEqual('is-positive')
  expect(metadata.versions['1.0.0'].scripts).not.toBeTruthy()
})

test('fetchFromRegistry fullMetadata', async () => {
  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json() as any // eslint-disable-line
  expect(metadata.name).toEqual('is-positive')
  expect(metadata.versions['1.0.0'].scripts).toBeTruthy()
})

test('authorization headers are removed before redirection if the target is on a different host', async () => {
  nock('http://registry.pnpm.io/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive')
    .reply(302, '', { location: 'http://registry.other.org/is-positive' })
  nock('http://registry.other.org/', { badheaders: ['authorization'] })
    .get('/is-positive')
    .reply(200, { ok: true })

  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry(
    'http://registry.pnpm.io/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  expect(await res.json()).toStrictEqual({ ok: true })
  expect(nock.isDone()).toBeTruthy()
})

test('authorization headers are not removed before redirection if the target is on the same host', async () => {
  nock('http://registry.pnpm.io/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive')
    .reply(302, '', { location: 'http://registry.pnpm.io/is-positive-new' })
  nock('http://registry.pnpm.io/', {
    reqheaders: { authorization: 'Bearer 123' },
  })
    .get('/is-positive-new')
    .reply(200, { ok: true })

  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })
  const res = await fetchFromRegistry(
    'http://registry.pnpm.io/is-positive',
    { authHeaderValue: 'Bearer 123' }
  )

  expect(await res.json()).toStrictEqual({ ok: true })
  expect(nock.isDone()).toBeTruthy()
})

test('switch to the correct agent for requests on redirect from http: to https:', async () => {
  const fetchFromRegistry = createFetchFromRegistry({ fullMetadata: true })

  // We can test this on any endpoint that redirects from http: to https:
  const { status } = await fetchFromRegistry('http://pnpm.io/pnpm.js')

  expect(status).toEqual(200)
})

test('fetch from registry with client certificate authentication', async () => {
  const randomPort = Math.floor(Math.random() * 10000 + 10000)
  const proxyServer = new ProxyServer(randomPort, {
    key: fs.readFileSync(path.join(CERTS_DIR, 'server-key.pem')),
    cert: fs.readFileSync(path.join(CERTS_DIR, 'server-crt.pem')),
    ca: fs.readFileSync(path.join(CERTS_DIR, 'ca-crt.pem')),
    rejectUnauthorized: true,
    requestCert: true,
  }, 'https://registry.npmjs.org/')

  await proxyServer.start()

  const sslConfigs = {
    [`//localhost:${randomPort}/`]: {
      ca: fs.readFileSync(path.join(CERTS_DIR, 'ca-crt.pem'), 'utf8'),
      cert: fs.readFileSync(path.join(CERTS_DIR, 'client-crt.pem'), 'utf8'),
      key: fs.readFileSync(path.join(CERTS_DIR, 'client-key.pem'), 'utf8'),
    },
  }

  const fetchFromRegistry = createFetchFromRegistry({
    sslConfigs,
    strictSsl: false,
  })

  try {
    const res = await fetchFromRegistry(`https://localhost:${randomPort}/is-positive`)
    const metadata = await res.json() as any // eslint-disable-line
    expect(metadata.name).toEqual('is-positive')
  } finally {
    await proxyServer.stop()
  }
})

test('fail if the client certificate is not provided', async () => {
  const randomPort = Math.floor(Math.random() * 10000 + 10000)
  const proxyServer = new ProxyServer(randomPort, {
    key: fs.readFileSync(path.join(CERTS_DIR, 'server-key.pem')),
    cert: fs.readFileSync(path.join(CERTS_DIR, 'server-crt.pem')),
    ca: fs.readFileSync(path.join(CERTS_DIR, 'ca-crt.pem')),
    rejectUnauthorized: true,
    requestCert: true,
  }, 'https://registry.npmjs.org/')

  await proxyServer.start()

  const fetchFromRegistry = createFetchFromRegistry({
    strictSsl: false,
  })

  let err!: Error & { code: string }
  try {
    await fetchFromRegistry(`https://localhost:${randomPort}/is-positive`, {
      retry: {
        retries: 0,
      },
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  } finally {
    await proxyServer.stop()
  }
  expect(err?.code).toMatch(/ECONNRESET|ERR_SSL_TLSV13_ALERT_CERTIFICATE_REQUIRED/)
})
