import { createHash } from 'node:crypto'
import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { publish } from '@pnpm/releasing.commands'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { checkPkgExists, DEFAULT_OPTS } from './utils/index.js'

interface ReceivedRequest {
  method: string
  url: string
  body: unknown
}

interface RegistryStub {
  url: string
  received: ReceivedRequest[]
  multiPublishStatusCode: number
  close: () => Promise<void>
}

/**
 * A minimal registry that 404s every packument read (so recursive publish considers every package
 * unpublished) and records `PUT /-/pnpm/v1/publish` requests.
 */
async function createRegistryStub (): Promise<RegistryStub> {
  const received: ReceivedRequest[] = []
  const stub: Pick<RegistryStub, 'received' | 'multiPublishStatusCode'> = {
    received,
    multiPublishStatusCode: 201,
  }
  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk) => chunks.push(chunk))
    req.on('end', () => {
      const rawBody = Buffer.concat(chunks)
      received.push({
        method: req.method!,
        url: req.url!,
        body: rawBody.length > 0 ? JSON.parse(rawBody.toString()) : undefined,
      })
      if (req.method === 'PUT' && req.url === '/-/pnpm/v1/publish') {
        res.statusCode = stub.multiPublishStatusCode
        res.setHeader('content-type', 'application/json')
        res.end(JSON.stringify({ ok: true, success: true }))
        return
      }
      res.statusCode = 404
      res.setHeader('content-type', 'application/json')
      res.end(JSON.stringify({ error: 'not found' }))
    })
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  const { port } = server.address() as AddressInfo
  return Object.assign(stub, {
    url: `http://127.0.0.1:${port}/`,
    close: () => new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err) reject(err)
        else resolve()
      })
    }),
  })
}

let registry: RegistryStub

beforeEach(async () => {
  registry = await createRegistryStub()
})

afterEach(async () => {
  await registry.close()
})

function batchPublishOpts () {
  return {
    ...DEFAULT_OPTS,
    batch: true,
    configByUri: {
      [registry.url.replace(/^http:/, '')]: {
        '@': { authToken: 'test-token' },
      },
    },
    dir: process.cwd(),
    gitChecks: false,
    recursive: true,
    registries: { default: registry.url },
    registry: registry.url,
  }
}

test('batch publish sends all packages in a single batch publish request', async () => {
  preparePackages([
    {
      name: '@pnpmtest/batch-pkg-1',
      version: '1.0.0',
    },
    {
      name: 'batch-pkg-2',
      version: '2.0.0',
    },
    {
      name: 'i-am-private',
      version: '1.0.0',
      private: true,
    },
  ])

  await publish.handler({
    ...batchPublishOpts(),
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    tag: 'next',
  }, [])

  const publishRequests = registry.received.filter(({ url }) => url === '/-/pnpm/v1/publish')
  expect(publishRequests).toHaveLength(1)
  expect(publishRequests[0].method).toBe('PUT')

  const { packages } = publishRequests[0].body as {
    packages: Array<{
      _id: string
      name: string
      'dist-tags': Record<string, string>
      versions: Record<string, { dist: { integrity: string, shasum: string, tarball: string } }>
      _attachments: Record<string, { content_type: string, data: string, length: number }>
    }>
  }
  expect(packages.map(({ name }) => name).sort()).toStrictEqual(['@pnpmtest/batch-pkg-1', 'batch-pkg-2'])

  const scopedPkg = packages.find(({ name }) => name === '@pnpmtest/batch-pkg-1')!
  expect(scopedPkg._id).toBe('@pnpmtest/batch-pkg-1')
  expect(scopedPkg['dist-tags']).toStrictEqual({ next: '1.0.0' })

  // The attachment is keyed by the full (scoped) name and its bytes match dist.integrity,
  // the same wire shape libnpmpublish produces for a single-package publish.
  const attachment = scopedPkg._attachments['@pnpmtest/batch-pkg-1-1.0.0.tgz']
  expect(attachment).toBeDefined()
  const tarballData = Buffer.from(attachment.data, 'base64')
  expect(attachment).toHaveLength(tarballData.length)
  const { dist } = scopedPkg.versions['1.0.0']
  expect(dist.integrity).toBe(`sha512-${createHash('sha512').update(tarballData).digest('base64')}`)
  expect(dist.shasum).toBe(createHash('sha1').update(tarballData).digest('hex'))
  expect(dist.tarball).toBe(`${registry.url}@pnpmtest/batch-pkg-1/-/@pnpmtest/batch-pkg-1-1.0.0.tgz`)
})

test('batch publish with --dry-run sends no request but reports the packages', async () => {
  preparePackages([
    {
      name: 'batch-dry-1',
      version: '1.0.0',
    },
    {
      name: 'batch-dry-2',
      version: '1.0.0',
    },
  ])

  const result = await publish.handler({
    ...batchPublishOpts(),
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dryRun: true,
    json: true,
  }, [])

  expect(registry.received.filter(({ url }) => url === '/-/pnpm/v1/publish')).toHaveLength(0)
  const publishedPackages = JSON.parse(result!.output!) as Array<{ name: string }>
  expect(publishedPackages.map(({ name }) => name).sort()).toStrictEqual(['batch-dry-1', 'batch-dry-2'])
})

test('batch publish fails with a clear error when the registry does not implement batch publish', async () => {
  preparePackages([
    {
      name: 'batch-unsupported-1',
      version: '1.0.0',
    },
  ])
  registry.multiPublishStatusCode = 404

  await expect(publish.handler({
    ...batchPublishOpts(),
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
  }, [])).rejects.toMatchObject({ code: 'ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED' })
})

test('batch publish against a real pnpr registry publishes every package', async () => {
  // This suffix is added to the package name to avoid issues if Jest reruns the test
  const SUFFIX = Date.now()
  const pkg1 = {
    name: `@pnpmtest/batch-e2e-project-1-${SUFFIX}`,
    version: '1.0.0',
  }
  const pkg2 = {
    name: `batch-e2e-project-2-${SUFFIX}`,
    version: '1.2.3',
  }
  preparePackages([pkg1, pkg2])

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    batch: true,
    configByUri: {
      [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
        '@': { basicAuth: REGISTRY_MOCK_CREDENTIALS },
      },
    },
    dir: process.cwd(),
    gitChecks: false,
    recursive: true,
  }, [])

  await checkPkgExists(pkg1.name, pkg1.version)
  await checkPkgExists(pkg2.name, pkg2.version)
})

test('--batch without --recursive is rejected', async () => {
  preparePackages([
    {
      name: 'batch-no-recursive',
      version: '1.0.0',
    },
  ])

  await expect(publish.handler({
    ...batchPublishOpts(),
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    recursive: false,
  }, [])).rejects.toMatchObject({ code: 'ERR_PNPM_BATCH_PUBLISH_REQUIRES_RECURSIVE' })

  expect(registry.received.filter(({ url }) => url === '/-/pnpm/v1/publish')).toHaveLength(0)
})
