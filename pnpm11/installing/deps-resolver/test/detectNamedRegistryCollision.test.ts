import { expect, test } from '@jest/globals'
import type { PackageResponse } from '@pnpm/store.controller-types'

import {
  detectNamedRegistryCollision,
  detectRegistryRevisionConflict,
  type ResolvedPackage,
} from '../lib/resolveDependencies.js'

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

test('an identical tarball URL still proves the two are one artifact', () => {
  // No integrity on either side yet, but both point at the same bytes.
  expect(() => {
    detectNamedRegistryCollision(
      { resolvedVia: 'named-registry', resolution: { tarball: 'https://example.com/foo-1.0.0.tgz' } } as unknown as ResolvedPackage,
      { body: { resolvedVia: 'npm-registry', resolution: { tarball: 'https://example.com/foo-1.0.0.tgz' } } } as unknown as PackageResponse
    )
  }).not.toThrow()
})

test('an identity that cannot be proven is treated as a collision', () => {
  // Without an integrity or a matching URL there is nothing to show the two
  // are the same artifact, so reusing one for the other could hand a
  // dependency the wrong registry's bytes.
  expect(() => {
    detectNamedRegistryCollision(
      resolved({ resolvedVia: 'named-registry', integrity: 'sha512-AAAA' }),
      { body: { resolvedVia: 'npm-registry', resolution: {} } } as unknown as PackageResponse
    )
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_NAMED_REGISTRY_PACKAGE_COLLISION' }))
})

test('different revisions of one package version conflict', () => {
  expect(() => {
    detectRegistryRevisionConflict(
      {
        name: 'foo',
        version: '1.0.0',
        resolution: { integrity: 'sha512-AAAA', revision: 1 },
      } as unknown as ResolvedPackage,
      {
        body: { resolution: { integrity: 'sha512-BBBB', revision: 2 } },
      } as unknown as PackageResponse
    )
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_REVISION_CONFLICT' }))
})

test('the same revision and integrity can be reused', () => {
  expect(() => {
    detectRegistryRevisionConflict(
      {
        name: 'foo',
        version: '1.0.0',
        resolution: { integrity: 'sha512-AAAA', revision: 1 },
      } as unknown as ResolvedPackage,
      {
        body: { resolution: { integrity: 'sha512-AAAA', revision: 1 } },
      } as unknown as PackageResponse
    )
  }).not.toThrow()
})
