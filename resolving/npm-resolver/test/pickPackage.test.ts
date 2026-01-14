import { pickPackage } from '../src/pickPackage.js'
import { type PackageMeta } from '@pnpm/registry.types'
import { FULL_META_DIR, ABBREVIATED_META_DIR } from '@pnpm/constants'
import { temporaryDirectory } from 'tempy'

function createMockMeta (options: {
  name: string
  version: string
  integrity: string
  scripts?: Record<string, string>
}): PackageMeta {
  return {
    name: options.name,
    'dist-tags': { latest: options.version },
    versions: {
      [options.version]: {
        name: options.name,
        version: options.version,
        ...(options.scripts && { scripts: options.scripts }),
        dist: {
          tarball: `https://registry.npmjs.org/${options.name}/-/${options.name}-${options.version}.tgz`,
          integrity: options.integrity,
          shasum: options.integrity.substring(0, 16),
        },
      },
    },
  }
}

describe('pickPackage', () => {
  test('should include metaDir in cache key for abbreviated metadata', async () => {
    const metaCache = new Map<string, PackageMeta>()
    const mockMeta = createMockMeta({
      name: 'test-package',
      version: '1.0.0',
      integrity: 'sha512-test',
    })

    let fetchCallCount = 0
    const mockFetch = async () => {
      fetchCallCount++
      return mockMeta
    }

    const ctx = {
      fetch: mockFetch,
      metaDir: ABBREVIATED_META_DIR,
      metaCache,
      cacheDir: temporaryDirectory(),
      offline: false,
      preferOffline: false,
    }

    const spec = { name: 'test-package', type: 'tag' as const, fetchSpec: 'latest' }
    const opts = {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      preferredVersionSelectors: undefined,
    }

    // It should fetch from the registry on the first call.
    await pickPackage(ctx, spec, opts)
    expect(fetchCallCount).toBe(1)
    expect(metaCache.has('test-package:metadata-v1.3')).toBe(true)

    // It should use cache on the second call.
    await pickPackage(ctx, spec, opts)
    expect(fetchCallCount).toBe(1)
  })

  test('should include metaDir in cache key for full metadata', async () => {
    const metaCache = new Map<string, PackageMeta>()
    const mockMeta = createMockMeta({
      name: 'test-package',
      version: '1.0.0',
      integrity: 'sha512-test',
    })

    let fetchCallCount = 0
    const mockFetch = async () => {
      fetchCallCount++
      return mockMeta
    }

    const ctx = {
      fetch: mockFetch,
      metaDir: FULL_META_DIR,
      metaCache,
      cacheDir: temporaryDirectory(),
      offline: false,
      preferOffline: false,
    }

    const spec = { name: 'test-package', type: 'tag' as const, fetchSpec: 'latest' }
    const opts = {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      metaDir: FULL_META_DIR,
      preferredVersionSelectors: undefined,
    }

    // It should fetch from the registry on the first call.
    await pickPackage(ctx, spec, opts)
    expect(fetchCallCount).toBe(1)
    expect(metaCache.has('test-package:metadata-full-v1.3')).toBe(true)

    // It should use cache on the second call.
    await pickPackage(ctx, spec, opts)
    expect(fetchCallCount).toBe(1)
  })

  test('should use different cache keys for abbreviated and full metadata', async () => {
    const metaCache = new Map<string, PackageMeta>()
    const abbreviatedMeta = createMockMeta({
      name: 'test-package',
      version: '1.0.0',
      integrity: 'sha512-abbreviated',
    })
    const fullMeta = createMockMeta({
      name: 'test-package',
      version: '1.0.0',
      integrity: 'sha512-full',
      scripts: { test: 'jest' },
    })

    let fetchCallCount = 0
    const mockFetch = async (
      _pkgName: string,
      _registry: string,
      _authHeaderValue: string | undefined,
      fullMetadata?: boolean
    ) => {
      fetchCallCount++
      return fullMetadata ? fullMeta : abbreviatedMeta
    }

    const ctx = {
      fetch: mockFetch,
      metaDir: ABBREVIATED_META_DIR,
      metaCache,
      cacheDir: temporaryDirectory(),
      offline: false,
      preferOffline: false,
    }

    const spec = { name: 'test-package', type: 'tag' as const, fetchSpec: 'latest' }

    // It should fetch abbreviated metadata from the registry.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      preferredVersionSelectors: undefined,
    })

    expect(fetchCallCount).toBe(1)
    expect(metaCache.has('test-package:metadata-v1.3')).toBe(true)

    // It should fetch full metadata separately when requested with different metaDir.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      metaDir: FULL_META_DIR,
      preferredVersionSelectors: undefined,
    })

    expect(fetchCallCount).toBe(2)
    expect(metaCache.has('test-package:metadata-v1.3')).toBe(true)
    expect(metaCache.has('test-package:metadata-full-v1.3')).toBe(true)

    // It should cache different metadata separately based on metaDir.
    const abbreviatedFromCache = metaCache.get('test-package:metadata-v1.3')
    const fullFromCache = metaCache.get('test-package:metadata-full-v1.3')

    expect(abbreviatedFromCache!.versions['1.0.0'].dist.integrity).toBe('sha512-abbreviated')
    expect(fullFromCache!.versions['1.0.0'].dist.integrity).toBe('sha512-full')
    expect(fullFromCache!.versions['1.0.0'].scripts).toBeDefined()
    expect(abbreviatedFromCache!.versions['1.0.0'].scripts).toBeUndefined()
  })

  test('should pass fullMetadata flag to fetch function', async () => {
    const metaCache = new Map<string, PackageMeta>()
    const mockMeta = createMockMeta({
      name: 'test-package',
      version: '1.0.0',
      integrity: 'sha512-test',
    })

    let fullMetadataParam: boolean | undefined
    const mockFetch = async (
      _pkgName: string,
      _registry: string,
      _authHeaderValue: string | undefined,
      fullMetadata?: boolean
    ) => {
      fullMetadataParam = fullMetadata
      return mockMeta
    }

    const ctx = {
      fetch: mockFetch,
      metaDir: ABBREVIATED_META_DIR,
      metaCache,
      cacheDir: temporaryDirectory(),
      offline: false,
      preferOffline: false,
    }

    const spec = { name: 'test-package', type: 'tag' as const, fetchSpec: 'latest' }

    // It should pass fullMetadata=false when using abbreviated metadata.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      preferredVersionSelectors: undefined,
    })
    expect(fullMetadataParam).toBe(false)

    // It should pass fullMetadata=true when using full metadata.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      metaDir: FULL_META_DIR,
      preferredVersionSelectors: undefined,
    })
    expect(fullMetadataParam).toBe(true)
  })
})
