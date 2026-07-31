import { expect, test } from '@jest/globals'
import type { PackageResponse } from '@pnpm/store.controller-types'

import { detectNamedRegistryCollision, type ResolvedPackage } from '../lib/resolveDependencies.js'

function resolved (opts: { resolvedVia: string, integrity: string }): ResolvedPackage {
  return {
    name: 'foo',
    version: '1.0.0',
    resolvedVia: opts.resolvedVia,
    resolution: { integrity: opts.integrity, tarball: 'https://example.com/foo-1.0.0.tgz' },
  } as unknown as ResolvedPackage
}

function response (opts: { resolvedVia: string, integrity: string }): PackageResponse {
  return {
    body: {
      resolvedVia: opts.resolvedVia,
      resolution: { integrity: opts.integrity, tarball: 'https://example.com/foo-1.0.0.tgz' },
    },
  } as unknown as PackageResponse
}

test('throws when a named registry serves a different artifact under an id another registry already resolved', () => {
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'npm-registry', integrity: 'sha512-AAAA' }),
      response({ resolvedVia: 'named-registry', integrity: 'sha512-BBBB' })
    )
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_NAMED_REGISTRY_PACKAGE_COLLISION' }))
})

test('throws regardless of which registry resolved first', () => {
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'named-registry', integrity: 'sha512-AAAA' }),
      response({ resolvedVia: 'npm-registry', integrity: 'sha512-BBBB' })
    )
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_NAMED_REGISTRY_PACKAGE_COLLISION' }))
})

test('the same artifact reached twice is not a collision', () => {
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'named-registry', integrity: 'sha512-AAAA' }),
      response({ resolvedVia: 'named-registry', integrity: 'sha512-AAAA' })
    )
  }).not.toThrow()
})

test('two ordinary registry resolutions are left alone', () => {
  // Nothing outside the named-registry path may be affected by this guard.
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'npm-registry', integrity: 'sha512-AAAA' }),
      response({ resolvedVia: 'npm-registry', integrity: 'sha512-BBBB' })
    )
  }).not.toThrow()
})

test('a resolution with no integrity yet cannot be judged', () => {
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'named-registry', integrity: 'sha512-AAAA' }),
      { body: { resolvedVia: 'npm-registry', resolution: {} } } as unknown as PackageResponse
    )
  }).not.toThrow()
})
