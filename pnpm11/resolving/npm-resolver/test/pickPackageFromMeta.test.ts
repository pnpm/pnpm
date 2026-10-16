import { expect, test } from '@jest/globals'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import { pickPackageFromMeta, pickVersionByVersionRange } from '../src/pickPackageFromMeta.js'

function meta (name: string | undefined, manifestName: string): PackageMeta {
  return {
    name,
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': { name: manifestName, version: '1.0.0' } as PackageInRegistry,
    },
  } as PackageMeta
}

function pick (packageMeta: PackageMeta, name: string): PackageInRegistry | null {
  return pickPackageFromMeta(
    pickVersionByVersionRange,
    { preferredVersionSelectors: undefined },
    packageMeta,
    { type: 'version', name, fetchSpec: '1.0.0' }
  )
}

test('metadata naming a different package is rejected', () => {
  expect(() => pick(meta('approved-pkg', '@untrusted/denied-pkg'), '@untrusted/denied-pkg'))
    .toThrow(expect.objectContaining({
      code: 'ERR_PNPM_REGISTRY_NAME_MISMATCH',
      message: 'The registry returned metadata for "approved-pkg" when "@untrusted/denied-pkg" was requested',
    }))
})

test('a manifest published without its scope resolves under the requested name', () => {
  expect(pick(meta('@acme/private', 'private'), '@acme/private')?.name).toBe('@acme/private')
})

test('a manifest naming another package resolves under the requested name', () => {
  expect(pick(meta('foo', 'approved-pkg'), 'foo')?.name).toBe('foo')
})

test('metadata without a name resolves under the requested name', () => {
  expect(pick(meta(undefined, 'approved-pkg'), 'foo')?.name).toBe('foo')
})
