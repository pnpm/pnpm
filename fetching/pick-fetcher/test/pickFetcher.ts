import { pickFetcher } from '@pnpm/pick-fetcher'
import { jest } from '@jest/globals'
import { type FetchFunction, type Fetchers } from '@pnpm/fetcher-base'
import { type CustomFetcher } from '@pnpm/hooks.types'

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
  'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
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

describe('custom fetcher support', () => {
  test('should use custom fetcher when canFetch returns true', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const customFetch = jest.fn(async () => mockFetchResult)
    const remoteTarball = jest.fn() as FetchFunction

    const customFetcher: Partial<CustomFetcher> = {
      canFetch: () => true,
      fetch: customFetch,
    }

    const mockFetchers = createMockFetchers({ remoteTarball })
    const fetcher = await pickFetcher(
      mockFetchers,
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(typeof fetcher).toBe('function')

    // Call the fetcher and verify it uses the custom fetch function
    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { tarball: 'http://example.com/package.tgz' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const result = await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(result).toBe(mockFetchResult)
    expect(customFetch).toHaveBeenCalledWith(
      mockCafs,
      { tarball: 'http://example.com/package.tgz' },
      mockFetchOpts,
      mockFetchers
    )
    expect(remoteTarball).not.toHaveBeenCalled()
  })

  test('should use custom fetcher when canFetch returns promise resolving to true', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const customFetch = jest.fn(async () => mockFetchResult)

    const customFetcher: Partial<CustomFetcher> = {
      canFetch: async () => Promise.resolve(true),
      fetch: customFetch,
    }

    const fetcher = await pickFetcher(
      createMockFetchers({}),
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(typeof fetcher).toBe('function')
  })

  test('should fall through to standard fetcher when canFetch returns false', async () => {
    const customFetch = jest.fn() as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const remoteTarball = jest.fn() as FetchFunction

    const customFetcher: Partial<CustomFetcher> = {
      canFetch: () => false,
      fetch: customFetch,
    }

    const fetcher = await pickFetcher(
      createMockFetchers({ remoteTarball }),
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(fetcher).toBe(remoteTarball)
    expect(customFetch).not.toHaveBeenCalled()
  })

  test('should skip custom fetcher without canFetch method', async () => {
    const remoteTarball = jest.fn() as FetchFunction

    const customFetcher: Partial<CustomFetcher> = {
      // No canFetch method
      fetch: jest.fn() as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const fetcher = await pickFetcher(
      createMockFetchers({ remoteTarball }),
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    expect(fetcher).toBe(remoteTarball)
  })

  test('should check custom fetchers in order and use first match', async () => {
    const mockFetchResult1 = { filesIndex: {}, manifest: { name: 'fetcher1', version: '1.0.0' }, requiresBuild: false }
    const mockFetchResult2 = { filesIndex: {}, manifest: { name: 'fetcher2', version: '1.0.0' }, requiresBuild: false }

    const fetcher1: Partial<CustomFetcher> = {
      canFetch: () => true,
      fetch: jest.fn(async () => mockFetchResult1),
    }

    const fetcher2: Partial<CustomFetcher> = {
      canFetch: () => true,
      fetch: jest.fn(async () => mockFetchResult2),
    }

    const fetcher = await pickFetcher(
      createMockFetchers({}),
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [fetcher1 as CustomFetcher, fetcher2 as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { tarball: 'http://example.com/package.tgz' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const result = await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(result).toBe(mockFetchResult1)
    expect(fetcher1.fetch).toHaveBeenCalled()
    expect(fetcher2.fetch).not.toHaveBeenCalled()
  })

  test('should handle custom resolution types', async () => {
    const mockFetchResult = { filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }
    const customFetch = jest.fn(async () => mockFetchResult)

    const customFetcher: Partial<CustomFetcher> = {
      canFetch: (pkgId: string, resolution: any) => resolution.type === 'custom:test', // eslint-disable-line @typescript-eslint/no-explicit-any
      fetch: customFetch,
    }

    const mockFetchers = createMockFetchers({})
    const fetcher = await pickFetcher(
      mockFetchers,
      { type: 'custom:test', customField: 'value' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
      }
    )

    const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockResolution = { type: 'custom:test', customField: 'value' } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    const mockFetchOpts = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any

    await fetcher(mockCafs, mockResolution, mockFetchOpts)

    expect(customFetch).toHaveBeenCalledWith(
      mockCafs,
      { type: 'custom:test', customField: 'value' },
      mockFetchOpts,
      mockFetchers
    )
  })

  test('should pass all fetch options to custom fetcher.fetch', async () => {
    const customFetch = jest.fn(async () => ({ filesIndex: {}, manifest: { name: 'test', version: '1.0.0' }, requiresBuild: false }))

    const customFetcher: Partial<CustomFetcher> = {
      canFetch: () => true,
      fetch: customFetch,
    }

    const mockFetchers = createMockFetchers({})
    const fetcher = await pickFetcher(
      mockFetchers,
      { tarball: 'http://example.com/package.tgz' },
      {
        customFetchers: [customFetcher as CustomFetcher],
        packageId: 'test-package@1.0.0',
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

    expect(customFetch).toHaveBeenCalledWith(mockCafs, mockResolution, mockFetchOpts, mockFetchers)
  })

  test('throws error for custom resolution type with no custom fetcher', async () => {
    // Custom resolution type without a matching custom fetcher
    const customResolution = {
      type: 'custom:cdn',
      cdnUrl: 'https://cdn.company.com/package.tgz',
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    await expect(
      pickFetcher(createMockFetchers({}), customResolution, {
        packageId: 'test-package@1.0.0',
      })
    ).rejects.toThrow('Cannot fetch dependency with custom resolution type "custom:cdn". Custom resolutions must be handled by custom fetchers.')
  })
})
