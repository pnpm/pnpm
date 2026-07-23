import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { resolveViaPnprServer } from '@pnpm/pnpr.client'

test('pnpr client serializes lockfile controls under the exact wire names', async () => {
  let requestBody: Record<string, unknown> | undefined
  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => {
      requestBody = JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>
      res.writeHead(200, { 'content-type': 'application/x-ndjson' })
      res.end(`${JSON.stringify({
        type: 'done',
        lockfile: {
          lockfileVersion: '9.0',
          importers: { '.': {} },
          packages: {},
          snapshots: {},
        },
        stats: { totalPackages: 0 },
      })}\n`)
    })
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))

  try {
    const { port } = server.address() as AddressInfo
    await resolveViaPnprServer({
      registryUrl: `http://127.0.0.1:${port}`,
      frozenLockfile: false,
      preferFrozenLockfile: false,
      ignoreManifestCheck: true,
      trustLockfile: true,
    })
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err == null) resolve()
        else reject(err)
      })
    })
  }

  const expectedControls = {
    frozenLockfile: false,
    preferFrozenLockfile: false,
    ignoreManifestCheck: true,
    trustLockfile: true,
  }
  expect(Object.fromEntries(
    Object.keys(expectedControls).map((key) => [key, requestBody?.[key]])
  )).toEqual(expectedControls)
  expect(requestBody).not.toHaveProperty('frozen_lockfile')
  expect(requestBody).not.toHaveProperty('prefer_frozen_lockfile')
  expect(requestBody).not.toHaveProperty('ignore_manifest_check')
  expect(requestBody).not.toHaveProperty('trust_lockfile')
})
