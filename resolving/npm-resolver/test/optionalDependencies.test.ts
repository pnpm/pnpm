/// <reference path="../../../__typings__/index.d.ts"/>
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { type Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'
import { setupMockAgent, teardownMockAgent, getMockAgent } from './utils/index.js'

const registries = {
  default: 'https://registry.npmjs.org/',
} satisfies Registries

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(() => {
  setupMockAgent()
})

describe('optional dependencies', () => {
  test('optional dependencies receive full metadata with libc field', async () => {
    // This test verifies the fix for https://github.com/pnpm/pnpm/issues/9950
    // Optional dependencies need full metadata to get the libc field for platform compatibility checks.
    const packageMeta = {
      name: 'platform-pkg',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'platform-pkg',
          version: '1.0.0',
          os: ['linux'],
          cpu: ['x64'],
          libc: ['glibc'],
          dist: {
            tarball: 'https://registry.npmjs.org/platform-pkg/-/platform-pkg-1.0.0.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    // Mock the full metadata request for optional dependency
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/platform-pkg', method: 'GET' })
      .reply(200, packageMeta)

    const { resolveFromNpm } = createResolveFromNpm({
      storeDir: temporaryDirectory(),
      cacheDir: temporaryDirectory(),
      registries,
    })

    const result = await resolveFromNpm(
      {
        alias: 'platform-pkg',
        bareSpecifier: '1.0.0',
        optional: true,
      },
      {}
    )

    expect(result!.manifest!.libc).toEqual(['glibc'])
    expect(result!.manifest!.os).toEqual(['linux'])
    expect(result!.manifest!.cpu).toEqual(['x64'])
  })

  test('abbreviated and full metadata are cached separately', async () => {
    // Abbreviated metadata doesn't include scripts, full metadata does.
    // When resolving the same package first as regular, then as optional,
    // we should get different metadata from each request.
    const abbreviatedMeta = {
      name: 'cache-test',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'cache-test',
          version: '1.0.0',
          dist: {
            tarball: 'https://registry.npmjs.org/cache-test/-/cache-test-1.0.0.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }
    const fullMeta = {
      name: 'cache-test',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'cache-test',
          version: '1.0.0',
          scripts: {
            test: 'jest',
            build: 'tsc',
          },
          dist: {
            tarball: 'https://registry.npmjs.org/cache-test/-/cache-test-1.0.0.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    const mockPool = getMockAgent()!.get(registries.default.replace(/\/$/, ''))
    // First request: abbreviated metadata for regular dependency
    mockPool.intercept({ path: '/cache-test', method: 'GET' }).reply(200, abbreviatedMeta)
    // Second request: full metadata for optional dependency
    mockPool.intercept({ path: '/cache-test', method: 'GET' }).reply(200, fullMeta)

    const cacheDir = temporaryDirectory()

    const { resolveFromNpm } = createResolveFromNpm({
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    // Resolve as regular dependency - should get abbreviated metadata
    const regularResult = await resolveFromNpm(
      { alias: 'cache-test', bareSpecifier: '1.0.0' },
      {}
    )
    expect(regularResult!.manifest!.scripts).toBeUndefined()

    // Resolve as optional dependency - should get full metadata (separate cache entry)
    const optionalResult = await resolveFromNpm(
      { alias: 'cache-test', bareSpecifier: '1.0.0', optional: true },
      {}
    )
    expect(optionalResult!.manifest!.scripts).toEqual({ test: 'jest', build: 'tsc' })
  })
})
