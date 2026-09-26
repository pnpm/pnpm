import http from 'node:http'
import https from 'node:https'
import type { AddressInfo } from 'node:net'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import { getProxyAgent } from '@pnpm/network.proxy-agent'
import { createProxy } from 'proxy'

describe('untrusted certificate', () => {
  const proxy = createProxy(http.createServer())
  let proxyPort: number
  beforeAll(async () => {
    await new Promise<void>((resolve) => proxy.listen(0, '127.0.0.1', resolve))
    proxyPort = (proxy.address() as AddressInfo).port
  })
  afterAll(async () => {
    await new Promise((resolve) => proxy.close(resolve))
  })
  it('should not throw an error if strictSsl is set to false', async () => {
    const url = 'https://self-signed.badssl.com'
    const agent = getProxyAgent(url, {
      httpsProxy: `http://127.0.0.1:${proxyPort}`,
      strictSsl: false,
    })
    await get(url, agent)
  })
  it('should throw an error if strictSsl is not set', async () => {
    const url = 'https://self-signed.badssl.com'
    const agent = getProxyAgent(url, {
      httpsProxy: `http://127.0.0.1:${proxyPort}`,
      strictSsl: true,
    })
    await expect(get(url, agent)).rejects.toThrow(/self[- ]signed certificate/)
  })
})

async function get (url: string, agent: http.Agent | undefined): Promise<void> {
  return new Promise((resolve, reject) => {
    https.get(url, { agent }, (res) => {
      res.resume()
      res.on('end', resolve)
    }).on('error', reject)
  })
}
