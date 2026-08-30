import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { type PnprProject, resolveViaPnprServer, type ResolveViaPnprServerOptions } from '@pnpm/pnpr.client'

interface CapturedResolveRequest {
  projects: Array<Record<string, unknown>>
  resolutionMode?: string
  patchedDependencies?: Record<string, string>
  packageExtensions?: Record<string, unknown>
  allowUnusedPatches?: boolean
  updatePatches?: boolean
}

const resolverSettingNames = ['autoInstallPeers', 'dedupePeers', 'excludeLinksFromLockfile'] as const

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
  const projects: PnprProject[] = [
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

test('omits absent identity fields and current lockfile resolution settings', async () => {
  const request = await captureResolveRequest({ dependencies: {} })

  expect(Object.hasOwn(request.projects[0], 'name')).toBe(false)
  expect(Object.hasOwn(request.projects[0], 'version')).toBe(false)
  for (const setting of resolverSettingNames) {
    expect(Object.hasOwn(request, setting)).toBe(false)
  }
})

test.each(resolverSettingNames)('serializes %s independently', async (setting) => {
  await Promise.all([true, false].map(async value => {
    const request = await captureResolveRequest({ dependencies: {}, [setting]: value })
    expect(request).toMatchObject({ [setting]: value })
    for (const key of resolverSettingNames) {
      expect(Object.hasOwn(request, key)).toBe(key === setting)
    }
  }))
})

// The mode decides which version every pick lands on, so a server resolving
// on the client's behalf has to be told it — omitted, it resolves `highest`
// and hands back a lockfile the client would never have written.
test.each(['time-based', 'lowest-direct', 'highest'] as const)('serializes resolutionMode %s', async (resolutionMode) => {
  const request = await captureResolveRequest({ dependencies: {}, resolutionMode })

  expect(request).toMatchObject({ resolutionMode })
})

test('omits resolutionMode when the caller has none', async () => {
  const request = await captureResolveRequest({ dependencies: {} })

  expect(Object.hasOwn(request, 'resolutionMode')).toBe(false)
})

test('serializes patch hashes and package extensions', async () => {
  const request = await captureResolveRequest({
    dependencies: {},
    patchedDependencies: { 'foo@1.0.0': 'abc123' },
    packageExtensions: {
      'foo@1.0.0': { dependencies: { bar: '1.0.0' } },
    },
    allowUnusedPatches: true,
  })

  expect(request).toMatchObject({
    patchedDependencies: { 'foo@1.0.0': 'abc123' },
    packageExtensions: {
      'foo@1.0.0': { dependencies: { bar: '1.0.0' } },
    },
    allowUnusedPatches: true,
  })
})

test.each([
  ['omits', undefined],
  ['changes', { 'foo@1.0.0': 'different-hash' }],
] as const)('rejects a server that %s the requested patch metadata', async (_behavior, patchedDependencies) => {
  await expect(captureResolveRequest({
    dependencies: {},
    patchedDependencies: { 'foo@1.0.0': 'abc123' },
  }, {
    lockfileVersion: '9.0',
    importers: { '.': {} },
    ...(patchedDependencies == null ? {} : { patchedDependencies }),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_PNPR_TRANSFORM_METADATA_MISMATCH',
    message: expect.stringContaining('returned patchedDependencies that do not match the request'),
  })
})

test.each([
  ['omits', undefined],
  ['changes', 'sha256-different-checksum'],
] as const)('rejects a server that %s the requested package extension metadata', async (_behavior, packageExtensionsChecksum) => {
  await expect(captureResolveRequest({
    dependencies: {},
    packageExtensions: {
      'foo@1.0.0': { dependencies: { bar: '1.0.0' } },
    },
  }, {
    lockfileVersion: '9.0',
    importers: { '.': {} },
    ...(packageExtensionsChecksum == null ? {} : { packageExtensionsChecksum }),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_PNPR_TRANSFORM_METADATA_MISMATCH',
    message: expect.stringContaining('returned packageExtensionsChecksum that does not match the request'),
  })
})

test.each([true, false])('serializes updatePatches %s', async (updatePatches) => {
  const request = await captureResolveRequest({ dependencies: {}, updatePatches })

  expect(request).toMatchObject({ updatePatches })
})

test('omits updatePatches when the caller has none', async () => {
  const request = await captureResolveRequest({ dependencies: {} })

  expect(Object.hasOwn(request, 'updatePatches')).toBe(false)
})

async function captureResolveRequest (
  options: Omit<ResolveViaPnprServerOptions, 'registryUrl'>,
  returnedLockfile?: Record<string, unknown>
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
      lockfile: returnedLockfile ?? {
        lockfileVersion: '9.0',
        importers: { '.': {} },
        patchedDependencies: capturedRequest.patchedDependencies,
        packageExtensionsChecksum: hashObjectNullableWithPrefix(capturedRequest.packageExtensions),
      },
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
