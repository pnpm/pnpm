///<reference path="../../../typings/index.d.ts"/>
import proxiquire = require('proxyquire')
import test = require('tape')
import getProcessEnv from '../lib/getProcessEnv'

const MockHttp = mockHttpAgent('http')
MockHttp['HttpsAgent'] = mockHttpAgent('https')
const agent = proxiquire('../lib/index.js', {
  'agentkeepalive': MockHttp,
  'https-proxy-agent': mockHttpAgent('https-proxy'),
}).default

function mockHttpAgent (type: string) {
  return function Agent (opts: any) { // tslint:disable-line:no-any
    return Object.assign({}, opts, { __type: type })
  }
}

test('extracts process env variables', t => {
  process.env = { TEST_ENV: 'test', ANOTHER_ENV: 'no' }

  t.deepEqual(getProcessEnv('test_ENV'), 'test', 'extracts single env')

  t.deepEqual(
    getProcessEnv(['not_existing_env', 'test_ENV', 'another_env']),
    'test',
    'extracts env from array of env names'
  )
  t.end()
})

const OPTS = {
  agent: null,
  ca: 'ca',
  cert: 'cert',
  key: 'key',
  localAddress: 'localAddress',
  maxSockets: 5,
  strictSSL: 'strictSSL',
  timeout: 5,
}

test('all expected options passed down to HttpAgent', t => {
  t.deepEqual(agent('http://foo.com/bar', OPTS), {
    __type: 'http',
    localAddress: 'localAddress',
    maxSockets: 5,
    timeout: 6,
  }, 'only expected options passed to HttpAgent')
  t.end()
})

test('all expected options passed down to HttpsAgent', t => {
  t.deepEqual(agent('https://foo.com/bar', OPTS), {
    __type: 'https',
    ca: 'ca',
    cert: 'cert',
    key: 'key',
    localAddress: 'localAddress',
    maxSockets: 5,
    rejectUnauthorized: 'strictSSL',
    timeout: 6,
  }, 'only expected options passed to HttpsAgent')
  t.end()
})

test('all expected options passed down to proxy agent', t => {
  const opts = Object.assign({
    proxy: 'https://user:pass@my.proxy:1234/foo',
  }, OPTS)
  t.deepEqual(agent('https://foo.com/bar', opts), {
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
    rejectUnauthorized: 'strictSSL',
    timeout: 6,
  }, 'only expected options passed to https proxy')
  t.end()
})
