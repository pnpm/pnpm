import { pickFetcher } from '@pnpm/pick-fetcher'
import { jest } from '@jest/globals'
import { type FetchFunction, type Fetchers } from '@pnpm/fetcher-base'
import { type Adapter } from '@pnpm/hooks.types'

// Helper to create a mock Fetchers object with only the needed fetcher
function createMockFetchers (partial: Partial<Fetchers>): Fetchers {
  const noop = jest.fn() as FetchFunction
  return {
    localTarball: noop,
    remoteTarball: noop,
    gitHostedTarball: noop,
    directory: noop as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    git: noop as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    binary: noop as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    ...partial,
  }
}

test('should pick localTarball fetcher', async () => {
  const localTarball = jest.fn() as FetchFunction
  const fetcher = await pickFetcher(createMockFetchers({ localTarball }), { tarball: 'file:is-positive-1.0.0.tgz' })
  expect(fetcher).toBe(localTarball)
})

test('should pick remoteTarball fetcher', async () => {
  const remoteTarball = jest.fn() as FetchFunction
  const fetcher = await pickFetcher(createMockFetchers({ remoteTarball }), { tarball: 'is-positive-1.0.0.tgz' })
  expect(fetcher).toBe(remoteTarball)
})

test.each([
  'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
  'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
  'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz',
])('should pick gitHostedTarball fetcher', async (tarball) => {
  const gitHostedTarball = jest.fn() as FetchFunction
  const fetcher = await pickFetcher(createMockFetchers({ gitHostedTarball }), { tarball })
  expect(fetcher).toBe(gitHostedTarball)
})

test('should fail to pick fetcher if the type is not defined', async () => {
  await expect(async () => {
    // This test specifically needs an incomplete Fetchers object to test error handling
    await pickFetcher({} as any, { type: 'directory', directory: expect.anything() } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  }).rejects.toThrow('Fetching for dependency type "directory" is not supported')
})

describe('adapter.fetch support', () => {
  test('should use adapter.fetch when canFetch returns true', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const adapterFetch = jest.fn(async () => mockFetchResult)
    const remoteTarball = jest.fn() as FetchFunction

    const adapter: Partial<Adapter> = {
      canFetch: () => true,
      fetch: adapterFetch,
    }

    const mockFetchers = createMockFetchers({ remoteTarball })
    const fetcher = await pickFetcher(
      mockFetchers,
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(typeof fetcher).toBe('function')

    // Call the fetcher and verify it uses adapter.fetch
    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { tarball: 'http://example.com/package.tgz' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const result = await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(result).toBe(mockFetchResult)
    expect(adapterFetch).toHaveBeenCalledWith(
      mockCafs,
      { tarball: 'http://example.com/package.tgz' },
      mockFetchOpts,
      mockFetchers
    )
    expect(remoteTarball).not.toHaveBeenCalled()
  })

  test('should use adapter.fetch when canFetch returns promise resolving to true', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const adapterFetch = jest.fn(async () => mockFetchResult)

    const adapter: Partial<Adapter> = {
      canFetch: async () => Promise.resolve(true),
      fetch: adapterFetch,
    }

    const fetcher = await pickFetcher(
      createMockFetchers({}),
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(typeof fetcher).toBe('function')
  })

  test('should fall through to standard fetcher when canFetch returns false', async () => {
    const adapterFetch = jest.fn() as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const remoteTarball = jest.fn() as FetchFunction

    const adapter: Partial<Adapter> = {
      canFetch: () => false,
      fetch: adapterFetch,
    }

    const fetcher = await pickFetcher(
      createMockFetchers({ remoteTarball }),
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(fetcher).toBe(remoteTarball)
    expect(adapterFetch).not.toHaveBeenCalled()
  })

  test('should skip adapter without canFetch method', async () => {
    const remoteTarball = jest.fn() as FetchFunction

    const adapter: Partial<Adapter> = {
      // No canFetch method
      fetch: jest.fn() as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const fetcher = await pickFetcher(
      createMockFetchers({ remoteTarball }),
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(fetcher).toBe(remoteTarball)
  })

  test('should check adapters in order and use first match', async () => {
    const mockFetchResult1 = { filesIndex: {}, manifest: { name: 'adapter1', version: '1.0.0' }, requiresBuild: false }
    const mockFetchResult2 = { filesIndex: {}, manifest: { name: 'adapter2', version: '1.0.0' }, requiresBuild: false }

    const adapter1: Partial<Adapter> = {
      canFetch: () => true,
      fetch: jest.fn(async () => mockFetchResult1),
    }

    const adapter2: Partial<Adapter> = {
      canFetch: () => true,
      fetch: jest.fn(async () => mockFetchResult2),
    }

    const fetcher = await pickFetcher(
      createMockFetchers({}),
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter1 as Adapter, adapter2 as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { tarball: 'http://example.com/package.tgz' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const result = await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(result).toBe(mockFetchResult1)
    expect(adapter1.fetch).toHaveBeenCalled()
    expect(adapter2.fetch).not.toHaveBeenCalled()
  })

  test('should require packageId for adapter.fetch', async () => {
    const remoteTarball = jest.fn() as FetchFunction

    const adapter: Partial<Adapter> = {
      canFetch: () => true,
      fetch: jest.fn() as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const fetcher = await pickFetcher(
      createMockFetchers({ remoteTarball }),
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        // No packageId
      }
    )

    // Should fall back to standard fetcher without packageId
    expect(fetcher).toBe(remoteTarball)
  })

  test('should handle custom resolution types', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const adapterFetch = jest.fn(async () => mockFetchResult)

    const adapter: Partial<Adapter> = {
      canFetch: (pkgId: string, resolution: any) => resolution.type === '@test/custom', // eslint-disable-line @typescript-eslint/no-explicit-any
      fetch: adapterFetch,
    }

    const mockFetchers = createMockFetchers({})
    const fetcher = await pickFetcher(
      mockFetchers,
      { type: '@test/custom', customField: 'value' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
      }
    )

    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { type: '@test/custom', customField: 'value' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(adapterFetch).toHaveBeenCalledWith(
      mockCafs,
      { type: '@test/custom', customField: 'value' },
      mockFetchOpts,
      mockFetchers
    )
  })

  test('should pass all fetch options to adapter.fetch', async () => {
    const adapterFetch = jest.fn(async () => ({ filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }))

    const adapter: Partial<Adapter> = {
      canFetch: () => true,
      fetch: adapterFetch,
    }

    const mockFetchers = createMockFetchers({})
    const fetcher = await pickFetcher(
      mockFetchers,
      { tarball: 'http://example.com/package.tgz' },
      {
        adapters: [adapter as Adapter],
        packageId: 'test-package@1.0.0',
        lockfileDir: '/project',
      }
    )

    const mockCafs = { addFilesFromTarball: jest.fn() } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { tarball: 'http://example.com/package.tgz' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {
      onStart: jest.fn(),
      onProgress: jest.fn(),
      readManifest: true,
      filesIndexFile: 'index.json',
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(adapterFetch).toHaveBeenCalledWith(mockCafs, mockResolution, mockFetchOpts, mockFetchers)
  })
})
