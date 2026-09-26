import { expect, test } from '@jest/globals'
import { getProxyAgent } from '@pnpm/network.proxy-agent'
import { SocksProxyAgent } from 'socks-proxy-agent'

interface HttpsProxyAgentInternals {
  connectOpts: unknown
  proxy: URL
}

const OPTS = {
  agent: null,
  ca: 'ca',
  cert: 'cert',
  key: 'key',
  localAddress: 'localAddress',
  maxSockets: 5,
  strictSsl: true,
  timeout: 5,
}

test('all expected options passed down to proxy agent', () => {
  const opts = {
    httpsProxy: 'https://user:pass@my.proxy:1234/foo/',
    noProxy: 'qar.com, bar.com',
    ...OPTS,
  }
  const agent = getProxyAgent('https://foo.com/bar', opts) as unknown as HttpsProxyAgentInternals
  expect(agent.connectOpts).toEqual({
    ALPNProtocols: ['http/1.1'],
    auth: 'user:pass',
    ca: 'ca',
    cert: 'cert',
    host: 'my.proxy',
    key: 'key',
    localAddress: 'localAddress',
    maxSockets: 5,
    port: 1234,
    rejectUnauthorized: true,
    timeout: 6,
  })
  expect(agent.proxy.protocol).toBe('https:')
})

test('a socks proxy', () => {
  const opts = {
    httpsProxy: 'socks://user:pass@my.proxy:1234/foo',
    ...OPTS,
  }
  const agent = getProxyAgent('https://foo.com/bar', opts)
  expect(agent instanceof SocksProxyAgent).toBeTruthy()
  expect((agent as SocksProxyAgent).proxy).toEqual({
    host: 'my.proxy',
    port: 1234,
    type: 5,
  })
})

test('proxy credentials are decoded', () => {
  const opts = {
    httpsProxy: `https://${encodeURIComponent('use@!r')}:${encodeURIComponent('p#as*s')}@my.proxy:1234/foo`,
    ...OPTS,
  }
  const agent = getProxyAgent('https://foo.com/bar', opts) as unknown as HttpsProxyAgentInternals
  expect(agent.connectOpts).toEqual({
    ALPNProtocols: ['http/1.1'],
    auth: 'use@!r:p#as*s',
    ca: 'ca',
    cert: 'cert',
    host: 'my.proxy',
    key: 'key',
    localAddress: 'localAddress',
    maxSockets: 5,
    port: 1234,
    // protocol: 'https:',
    rejectUnauthorized: true,
    timeout: 6,
  })
  expect(agent.proxy.protocol).toBe('https:')
})

test('proxy credentials that are not URL-encoded are rejected', () => {
  const opts = {
    httpsProxy: 'https://use@!r:p#as*s@my.proxy:1234/foo',
    ...OPTS,
  }
  expect(() => getProxyAgent('https://foo.com/bar', opts)).toThrow("Couldn't parse proxy URL")
})
