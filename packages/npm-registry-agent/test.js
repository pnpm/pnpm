'use strict'

const test = require('tape')
const proxiquire = require('proxyquire')

const MockHttp = mockHttpAgent('http')
MockHttp.HttpsAgent = mockHttpAgent('https')
const agent = proxiquire('./index.js', {
  'agentkeepalive': MockHttp,
  'https-proxy-agent': mockHttpAgent('https-proxy')
})

function mockHttpAgent (type) {
  return function Agent (opts) {
    return Object.assign({}, opts, { __type: type })
  }
}

test('extracts process env variables', t => {
  process.env = { TEST_ENV: 'test', ANOTHER_ENV: 'no' }

  t.deepEqual(agent.getProcessEnv('test_ENV'), 'test', 'extracts single env')

  t.deepEqual(
    agent.getProcessEnv(['not_existing_env', 'test_ENV', 'another_env']),
    'test',
    'extracts env from array of env names'
  )
  t.end()
})

const OPTS = {
  agent: null,
  maxSockets: 5,
  ca: 'ca',
  cert: 'cert',
  key: 'key',
  localAddress: 'localAddress',
  strictSSL: 'strictSSL',
  timeout: 5
}

test('all expected options passed down to HttpAgent', t => {
  t.deepEqual(agent('http://foo.com/bar', OPTS), {
    __type: 'http',
    maxSockets: 5,
    localAddress: 'localAddress',
    timeout: 6
  }, 'only expected options passed to HttpAgent')
  t.end()
})

test('all expected options passed down to HttpsAgent', t => {
  t.deepEqual(agent('https://foo.com/bar', OPTS), {
    __type: 'https',
    ca: 'ca',
    cert: 'cert',
    key: 'key',
    maxSockets: 5,
    localAddress: 'localAddress',
    rejectUnauthorized: 'strictSSL',
    timeout: 6
  }, 'only expected options passed to HttpsAgent')
  t.end()
})

test('all expected options passed down to proxy agent', t => {
  const opts = Object.assign({
    proxy: 'https://user:pass@my.proxy:1234/foo'
  }, OPTS)
  t.deepEqual(agent('https://foo.com/bar', opts), {
    __type: 'https-proxy',
    host: 'my.proxy',
    port: '1234',
    protocol: 'https:',
    path: '/foo',
    auth: 'user:pass',
    ca: 'ca',
    cert: 'cert',
    key: 'key',
    maxSockets: 5,
    localAddress: 'localAddress',
    rejectUnauthorized: 'strictSSL',
    timeout: 6
  }, 'only expected options passed to https proxy')
  t.end()
})
