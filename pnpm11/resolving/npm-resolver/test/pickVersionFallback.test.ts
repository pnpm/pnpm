import { expect, test } from '@jest/globals'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import { pickVersionByVersionRange } from '../src/pickPackageFromMeta.js'

// Builds a meta whose versions hydrate through getters, like a lazily-loaded
// indexed mirror does, recording which versions were hydrated. A null manifest
// input models a corrupt fragment (hydrates to undefined).
function lazyMeta (manifests: Record<string, PackageInRegistry | null>): { meta: PackageMeta, hydrated: string[] } {
  const hydrated: string[] = []
  const versions: PackageMeta['versions'] = Object.create(null)
  for (const [version, manifest] of Object.entries(manifests)) {
    Object.defineProperty(versions, version, {
      enumerable: true,
      configurable: true,
      get (): PackageInRegistry | undefined {
        hydrated.push(version)
        return manifest ?? undefined
      },
    })
  }
  return {
    meta: { name: 'foo', 'dist-tags': {}, versions },
    hydrated,
  }
}

function manifest (version: string, deprecated?: string): PackageInRegistry {
  return { name: 'foo', version, deprecated } as PackageInRegistry
}

test('the deprecated fallback hydrates only versions that satisfy the range', () => {
  const { meta, hydrated } = lazyMeta({
    '1.0.0': manifest('1.0.0'),
    '2.0.0': manifest('2.0.0'),
    '2.0.1': manifest('2.0.1', 'use 3.x'),
  })

  expect(pickVersionByVersionRange({ meta, versionRange: '^2.0.0' })).toBe('2.0.0')
  expect(hydrated).not.toContain('1.0.0')
})

test('a corrupt manifest of the max satisfying version fails the pick instead of crashing', () => {
  const { meta } = lazyMeta({
    '1.0.0': manifest('1.0.0'),
    '2.0.0': null,
  })

  // The version is still chosen (its manifest hydrates to undefined, which the
  // picker's missing-manifest handling turns into a failed pick downstream);
  // the deprecated check must not crash on the missing manifest.
  expect(pickVersionByVersionRange({ meta, versionRange: '>=1.0.0' })).toBe('2.0.0')
})

test('a corrupt manifest is not offered as the non-deprecated alternative', () => {
  const { meta } = lazyMeta({
    '2.0.0': null,
    '2.0.1': manifest('2.0.1', 'use 3.x'),
  })

  expect(pickVersionByVersionRange({ meta, versionRange: '^2.0.0' })).toBe('2.0.1')
})
