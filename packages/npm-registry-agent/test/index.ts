/// <reference path="../../../typings/index.d.ts"/>
import agent from '@pnpm/npm-registry-agent'

jest.mock('agentkeepalive', () => {
  const MockHttp = mockHttpAgent('http')
  MockHttp['HttpsAgent'] = mockHttpAgent('https')
  return MockHttp
})
jest.mock('https-proxy-agent', () => mockHttpAgent('https-proxy'))

function mockHttpAgent (type: string) {
  return function Agent (opts: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
    return Object.assign({}, opts, { __type: type })
  }
}

const OPTS = {
  agent: null,
  ca: 'ca',
  cert: 'cert',
  key: 'key',
  localAddress: 'localAddress',
  maxSockets: 5,
  strictSSL: true,
  timeout: 5,
}

test('all expected options passed down to HttpAgent', () => {
  expect(agent('http://foo.com/bar', OPTS)).toEqual({
    __type: 'http',
    localAddress: 'localAddress',
    maxSockets: 5,
    timeout: 6,
  })
})

test('all expected options passed down to HttpsAgent', () => {
  expect(agent('https://foo.com/bar', OPTS)).toEqual({
    __type: 'https',
    ca: 'ca',
    cert: 'cert',
    key: 'key',
    localAddress: 'localAddress',
    maxSockets: 5,
    rejectUnauthorized: true,
    timeout: 6,
  })
})

test('all expected options passed down to proxy agent', () => {
  const opts = Object.assign({
    httpsProxy: 'https://user:pass@my.proxy:1234/foo',
  }, OPTS)
  expect(agent('https://foo.com/bar', opts)).toEqual({
    __type: 'https-proxy',
    auth: 'user:pass',
    ca: 'ca',
    cert: 'cert',
    host: 'my.proxy',
    key: 'key',
    localAddress: 'localAddress',
    maxSockets: 5,
    path: '/foo',
    port: '1234',
    protocol: 'https:',
    rejectUnauthorized: true,
    timeout: 6,
  })
})
