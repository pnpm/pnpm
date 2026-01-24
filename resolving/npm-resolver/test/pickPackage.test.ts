import { pickPackage } from '../src/pickPackage.js'
import { type PackageMeta } from '@pnpm/registry.types'
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
      fullMetadata: false,
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
    expect(metaCache.has('test-package')).toBe(true)

    // It should use cache on the second call.
    await pickPackage(ctx, spec, opts)
    expect(fetchCallCount).toBe(1)
  })

  test('should include fullMetadata in cache key for full metadata', async () => {
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
      fullMetadata: true,
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
    expect(metaCache.has('test-package:full')).toBe(true)

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
      opts: { fullMetadata?: boolean }
    ) => {
      fetchCallCount++
      return opts.fullMetadata ? fullMeta : abbreviatedMeta
    }

    const ctx = {
      fetch: mockFetch,
      fullMetadata: false,
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
    expect(metaCache.has('test-package')).toBe(true)

    // It should fetch full metadata separately when requested with optional=true.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      optional: true,
      preferredVersionSelectors: undefined,
    })

    expect(fetchCallCount).toBe(2)
    expect(metaCache.has('test-package')).toBe(true)
    expect(metaCache.has('test-package:full')).toBe(true)

    // It should cache different metadata separately based on fullMetadata.
    const abbreviatedFromCache = metaCache.get('test-package')
    const fullFromCache = metaCache.get('test-package:full')

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
      opts: { fullMetadata?: boolean }
    ) => {
      fullMetadataParam = opts.fullMetadata
      return mockMeta
    }

    const ctx = {
      fetch: mockFetch,
      fullMetadata: false,
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

    // It should pass fullMetadata=true when optional=true.
    await pickPackage(ctx, spec, {
      registry: 'https://registry.npmjs.org/',
      dryRun: false,
      optional: true,
      preferredVersionSelectors: undefined,
    })
    expect(fullMetadataParam).toBe(true)
  })
})
