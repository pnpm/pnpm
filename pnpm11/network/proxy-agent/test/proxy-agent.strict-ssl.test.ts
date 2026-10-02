import fs from 'node:fs'
import http from 'node:http'
import https from 'node:https'
import type { AddressInfo } from 'node:net'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import { getProxyAgent } from '@pnpm/network.proxy-agent'
import { createProxy } from 'proxy'

describe('untrusted certificate', () => {
  const proxy = createProxy(http.createServer())
  const server = https.createServer({
    key: fs.readFileSync(new URL('../../fetch/test/__certs__/server-key.pem', import.meta.url)),
    cert: fs.readFileSync(new URL('../../fetch/test/__certs__/server-crt.pem', import.meta.url)),
  }, (_req, res) => res.end('ok'))
  let proxyPort: number
  let url: string
  beforeAll(async () => {
    await new Promise<void>((resolve) => proxy.listen(0, '127.0.0.1', resolve))
    proxyPort = (proxy.address() as AddressInfo).port
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    url = `https://127.0.0.1:${(server.address() as AddressInfo).port}`
  })
  afterAll(async () => {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
    await new Promise((resolve) => proxy.close(resolve))
  })
  it('should not throw an error if strictSsl is set to false', async () => {
    const agent = getProxyAgent(url, {
      httpsProxy: `http://127.0.0.1:${proxyPort}`,
      strictSsl: false,
    })
    await get(url, agent)
  })
  it('should throw an error if strictSsl is set to true', async () => {
    const agent = getProxyAgent(url, {
      httpsProxy: `http://127.0.0.1:${proxyPort}`,
      strictSsl: true,
    })
    await expect(get(url, agent)).rejects.toHaveProperty('code', 'UNABLE_TO_VERIFY_LEAF_SIGNATURE')
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
