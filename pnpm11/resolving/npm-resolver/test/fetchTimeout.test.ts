import http from 'node:http'
import type { AddressInfo, Socket } from 'node:net'

import { afterEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'

import { fetchMetadataFromFromRegistry } from '../src/fetch.js'

interface StalledRegistry {
  url: string
  close: () => void
}

const registries: StalledRegistry[] = []

afterEach(() => {
  for (const registry of registries.splice(0)) registry.close()
})

/**
 * Accepts every request, runs `respond` on it, and never finishes the
 * response, so the fetch timeout is the only thing that can end the request.
 */
async function startStalledRegistry (respond: (res: http.ServerResponse) => void): Promise<string> {
  const sockets = new Set<Socket>()
  const server = http.createServer((_req, res) => {
    respond(res)
  })
  server.on('connection', (socket) => {
    sockets.add(socket)
    socket.on('close', () => sockets.delete(socket))
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  const registry: StalledRegistry = {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/`,
    close: () => {
      for (const socket of sockets) socket.destroy()
      server.close()
    },
  }
  registries.push(registry)
  return registry.url
}

async function fetchFromStalledRegistry (registry: string): Promise<unknown> {
  return fetchMetadataFromFromRegistry({
    fetch: createFetchFromRegistry({}),
    retry: { retries: 0 },
    timeout: 200,
    fetchWarnTimeoutMs: 10_000,
  }, 'stalled-pkg', { registry })
}

test('a registry that never answers fails with a timeout error', async () => {
  const registry = await startStalledRegistry(() => {})

  await expect(fetchFromStalledRegistry(registry)).rejects.toMatchObject({
    code: 'ERR_PNPM_META_FETCH_FAIL',
    message: `GET ${registry}stalled-pkg: timed out, no data received for 200ms`,
  })
})

test('a metadata body that stops arriving fails with a timeout error', async () => {
  const registry = await startStalledRegistry((res) => {
    res.writeHead(200, { 'content-type': 'application/json', 'content-length': '1000' })
    res.write('{"name":')
  })

  await expect(fetchFromStalledRegistry(registry)).rejects.toMatchObject({
    code: 'ERR_PNPM_META_FETCH_FAIL',
    message: `GET ${registry}stalled-pkg: timed out, no data received for 200ms`,
  })
})
