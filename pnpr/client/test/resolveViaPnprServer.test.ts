import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { resolveViaPnprServer, type ResolveViaPnprServerOptions } from '@pnpm/pnpr.client'

interface CapturedResolveRequest {
  projects: Array<Record<string, unknown>>
}

test('serializes name and version for the single-project compatibility options', async () => {
  const options = {
    name: 'app',
    version: '1.2.3',
    dependencies: {},
  }

  const request = await captureResolveRequest(options)

  expect(request.projects).toEqual([{
    dir: '.',
    name: 'app',
    version: '1.2.3',
    dependencies: {},
  }])
})

test('serializes name and version for every explicit project', async () => {
  const projects = [
    {
      dir: 'packages/app',
      name: 'app',
      version: '1.0.0',
      dependencies: { lib: 'workspace:*' },
    },
    {
      dir: 'packages/lib',
      name: 'lib',
      version: '2.0.0',
      dependencies: {},
    },
  ]

  const request = await captureResolveRequest({ projects })

  expect(request.projects).toEqual(projects)
})

test('omits absent identity fields instead of serializing null', async () => {
  const request = await captureResolveRequest({ dependencies: {} })

  expect(Object.hasOwn(request.projects[0], 'name')).toBe(false)
  expect(Object.hasOwn(request.projects[0], 'version')).toBe(false)
})

async function captureResolveRequest (
  options: Omit<ResolveViaPnprServerOptions, 'registryUrl'>
): Promise<CapturedResolveRequest> {
  let capturedRequest: CapturedResolveRequest | undefined
  const server = http.createServer(async (request, response) => {
    const chunks: Buffer[] = []
    for await (const chunk of request) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
    }
    capturedRequest = JSON.parse(Buffer.concat(chunks).toString('utf8')) as CapturedResolveRequest
    response.end(`${JSON.stringify({
      type: 'done',
      lockfile: { lockfileVersion: '9.0', importers: { '.': {} } },
      stats: { totalPackages: 0 },
    })}\n`)
  })

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      server.off('error', reject)
      resolve()
    })
  })

  try {
    const { port } = server.address() as AddressInfo
    await resolveViaPnprServer({
      ...options,
      registryUrl: `http://127.0.0.1:${port}/`,
    })
    if (capturedRequest == null) {
      throw new Error('The pnpr client did not send a resolve request')
    }
    return capturedRequest
  } finally {
    await new Promise<void>((resolve, reject) => {
      server.close((error) => error == null ? resolve() : reject(error))
    })
  }
}
