/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { MockAgent, setGlobalDispatcher, getGlobalDispatcher, type Dispatcher } from 'undici'
import { ProxyServer } from 'https-proxy-server-express'
import fs from 'fs'

const CERTS_DIR = path.join(import.meta.dirname, '__certs__')

let mockAgent: MockAgent
let originalDispatcher: Dispatcher

function setupMockAgent (): void {
  originalDispatcher = getGlobalDispatcher()
  mockAgent = new MockAgent()
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
}

function teardownMockAgent (): void {
  setGlobalDispatcher(originalDispatcher)
}

test('fetchFromRegistry', async () => {
  // This test uses real network - no mock needed
  const fetchFromRegistry = createFetchFromRegistry({})
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json() as any // eslint-disable-line
  expect(metadata.name).toBe('is-positive')
  expect(metadata.versions['1.0.0'].scripts).not.toBeTruthy()
})

test('fetchFromRegistry fullMetadata', async () => {
  // This test uses real network - no mock needed
  const fetchFromRegistry = createFetchFromRegistry({})
  const res = await fetchFromRegistry('https://registry.npmjs.org/is-positive', { fullMetadata: true })
  const metadata = await res.json() as any // eslint-disable-line
  expect(metadata.name).toBe('is-positive')
  expect(metadata.versions['1.0.0'].scripts).toBeTruthy()
})

test('authorization headers are removed before redirection if the target is on a different host', async () => {
  setupMockAgent()
  try {
    const mockPool1 = mockAgent.get('http://registry.pnpm.io')
    mockPool1.intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { authorization: 'Bearer 123' },
    }).reply(302, '', { headers: { location: 'http://registry.other.org/is-positive' } })

    const mockPool2 = mockAgent.get('http://registry.other.org')
    mockPool2.intercept({
      path: '/is-positive',
      method: 'GET',
    }).reply(200, { ok: true }, { headers: { 'content-type': 'application/json' } })

    const fetchFromRegistry = createFetchFromRegistry({})
    const res = await fetchFromRegistry(
      'http://registry.pnpm.io/is-positive',
      { authHeaderValue: 'Bearer 123' }
    )

    expect(await res.json()).toStrictEqual({ ok: true })
  } finally {
    teardownMockAgent()
  }
})

test('authorization headers are not removed before redirection if the target is on the same host', async () => {
  setupMockAgent()
  try {
    const mockPool = mockAgent.get('http://registry.pnpm.io')
    mockPool.intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { authorization: 'Bearer 123' },
    }).reply(302, '', { headers: { location: 'http://registry.pnpm.io/is-positive-new' } })

    mockPool.intercept({
      path: '/is-positive-new',
      method: 'GET',
      headers: { authorization: 'Bearer 123' },
    }).reply(200, { ok: true }, { headers: { 'content-type': 'application/json' } })

    const fetchFromRegistry = createFetchFromRegistry({})
    const res = await fetchFromRegistry(
      'http://registry.pnpm.io/is-positive',
      { authHeaderValue: 'Bearer 123' }
    )

    expect(await res.json()).toStrictEqual({ ok: true })
  } finally {
    teardownMockAgent()
  }
})

test('switch to the correct agent for requests on redirect from http: to https:', async () => {
  // This test uses real network - no mock needed
  const fetchFromRegistry = createFetchFromRegistry({})

  // We can test this on any endpoint that redirects from http: to https:
  const { status } = await fetchFromRegistry('http://pnpm.io/pnpm.js')

  expect(status).toBe(200)
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
    expect(metadata.name).toBe('is-positive')
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

  let err!: Error & { code?: string, cause?: { code?: string } }
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
  // undici errors may have the code in err.cause.code
  const errorCode = err?.code ?? err?.cause?.code
  expect(errorCode).toMatch(/ECONNRESET|ERR_SSL_TLSV13_ALERT_CERTIFICATE_REQUIRED|UNABLE_TO_VERIFY_LEAF_SIGNATURE|UND_ERR_SOCKET/)
})

test('redirect to protocol-relative URL', async () => {
  setupMockAgent()
  try {
    const mockPool1 = mockAgent.get('http://registry.pnpm.io')
    mockPool1.intercept({
      path: '/foo',
      method: 'GET',
    }).reply(302, '', { headers: { location: '//registry.other.org/foo' } })

    const mockPool2 = mockAgent.get('http://registry.other.org')
    mockPool2.intercept({
      path: '/foo',
      method: 'GET',
    }).reply(200, { ok: true }, { headers: { 'content-type': 'application/json' } })

    const fetchFromRegistry = createFetchFromRegistry({})
    const res = await fetchFromRegistry(
      'http://registry.pnpm.io/foo'
    )

    expect(await res.json()).toStrictEqual({ ok: true })
  } finally {
    teardownMockAgent()
  }
})

test('redirect to relative URL', async () => {
  setupMockAgent()
  try {
    const mockPool = mockAgent.get('http://registry.pnpm.io')
    mockPool.intercept({
      path: '/bar/baz',
      method: 'GET',
    }).reply(302, '', { headers: { location: '../foo' } })

    mockPool.intercept({
      path: '/foo',
      method: 'GET',
    }).reply(200, { ok: true }, { headers: { 'content-type': 'application/json' } })

    const fetchFromRegistry = createFetchFromRegistry({})
    const res = await fetchFromRegistry(
      'http://registry.pnpm.io/bar/baz'
    )

    expect(await res.json()).toStrictEqual({ ok: true })
  } finally {
    teardownMockAgent()
  }
})

test('redirect to relative URL when request pkg.pr.new link', async () => {
  setupMockAgent()
  try {
    const mockPool = mockAgent.get('https://pkg.pr.new')
    mockPool.intercept({
      path: '/vue@14175',
      method: 'GET',
    }).reply(302, '', { headers: { location: '/vuejs/core/vue@14182' } })

    mockPool.intercept({
      path: '/vuejs/core/vue@14182',
      method: 'GET',
    }).reply(302, '', { headers: { location: '/vuejs/core/vue@82a13bb6faaa9f77a06b57e69e0934b9f620f333' } })

    mockPool.intercept({
      path: '/vuejs/core/vue@82a13bb6faaa9f77a06b57e69e0934b9f620f333',
      method: 'GET',
    }).reply(200, { ok: true }, { headers: { 'content-type': 'application/json' } })

    const fetchFromRegistry = createFetchFromRegistry({})
    const res = await fetchFromRegistry(
      'https://pkg.pr.new/vue@14175'
    )

    expect(await res.json()).toStrictEqual({ ok: true })
  } finally {
    teardownMockAgent()
  }
})

test('redirect without location header throws error', async () => {
  setupMockAgent()
  try {
    const mockPool = mockAgent.get('http://registry.pnpm.io')
    mockPool.intercept({
      path: '/missing-location',
      method: 'GET',
    }).reply(302, 'found')

    const fetchFromRegistry = createFetchFromRegistry({})
    await expect(fetchFromRegistry(
      'http://registry.pnpm.io/missing-location'
    )).rejects.toThrow(/Redirect location header missing/)
  } finally {
    teardownMockAgent()
  }
})
