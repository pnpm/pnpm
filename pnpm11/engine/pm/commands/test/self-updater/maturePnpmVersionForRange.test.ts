import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

const actualModule = await import('@pnpm/cli.meta')
const mockPackageManager = {
  name: 'pnpm',
  version: '9.1.0',
}
jest.unstable_mockModule('@pnpm/cli.meta', () => {
  return {
    ...actualModule,
    packageManager: mockPackageManager,
  }
})
const { maturePnpmVersionForRange } = await import('@pnpm/engine.pm.commands')

const REGISTRY = 'https://registry.npmjs.org/'
const OLDER = new Date(Date.now() - 72 * 60 * 60 * 1000).toISOString()
const OLD = new Date(Date.now() - 48 * 60 * 60 * 1000).toISOString()
const FRESH = new Date(Date.now() - 8 * 60 * 60 * 1000).toISOString()

beforeEach(async () => {
  mockPackageManager.version = '9.1.0'
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('an immature running pnpm gives way to the newest mature version in the range', async () => {
  servePnpm({ '9.0.0': OLD, '9.0.1': OLD, '9.1.0': FRESH })

  expect(await maturePnpmVersionForRange(options(), '^9.0.0')).toBe('9.0.1')
})

test('a mature running pnpm is recorded even when a newer one is mature', async () => {
  mockPackageManager.version = '9.0.0'
  servePnpm({ '9.0.0': OLD, '9.1.0': OLD })

  expect(await maturePnpmVersionForRange(options(), '^9.0.0')).toBe('9.0.0')
})

test('the running pnpm is kept when nothing in the range is mature', async () => {
  servePnpm({ '8.0.0': OLD, '9.0.0': FRESH, '9.1.0': FRESH })

  expect(await maturePnpmVersionForRange(options(), '^9.0.0')).toBe('9.1.0')
})

test('an excluded running pnpm is recorded', async () => {
  servePnpm({ '9.0.0': OLD, '9.1.0': FRESH })

  expect(await maturePnpmVersionForRange(options({ minimumReleaseAgeExclude: ['pnpm@9.1.0'] }), '^9.0.0')).toBe('9.1.0')
})

test('the range fallback skips a trust downgrade', async () => {
  mockPackageManager.version = '9.2.0'
  servePnpm({ '9.0.0': OLDER, '9.1.0': OLD, '9.2.0': FRESH }, {
    '9.0.0': {
      _npmUser: { name: 'alice', trustedPublisher: { id: 'github', oidcConfigId: 'release' } },
      dist: { attestations: { provenance: { predicateType: 'https://slsa.dev/provenance/v1' } } },
    },
  })

  expect(await maturePnpmVersionForRange({ ...options(), trustPolicy: 'no-downgrade' }, '^9.0.0')).toBe('9.0.0')
})

test('the registry is not asked without a cutoff', async () => {
  expect(await maturePnpmVersionForRange(options({ minimumReleaseAge: 0 }), '^9.0.0')).toBe('9.1.0')
})

function options (overrides: { minimumReleaseAge?: number, minimumReleaseAgeExclude?: string[] } = {}) {
  const dir = tempDir(false)
  return {
    cacheDir: path.join(dir, '.cache'),
    dir,
    minimumReleaseAge: 24 * 60,
    ...overrides,
  }
}

function servePnpm (time: Record<string, string>, extra: Record<string, { _npmUser?: object, dist?: object }> = {}): void {
  const versions = Object.keys(time)
  const metadata = {
    name: 'pnpm',
    'dist-tags': { latest: versions.at(-1) },
    versions: Object.fromEntries(versions.map((version) => [
      version,
      {
        name: 'pnpm',
        version,
        _npmUser: extra[version]?._npmUser,
        dist: {
          shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
          tarball: `${REGISTRY}pnpm/-/pnpm-${version}.tgz`,
          integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          ...extra[version]?.dist,
        },
      },
    ])),
    time,
  }
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)
    .persist()
}
