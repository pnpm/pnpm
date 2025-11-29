import { pickFetcher } from '@pnpm/pick-fetcher'
import { jest } from '@jest/globals'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { fixtures } from '@pnpm/test-fixtures'
import { temporaryDirectory } from 'tempy'
import path from 'path'
import nock from 'nock'
import type { Cafs } from '@pnpm/cafs-types'
import type { FetchFunction, Fetchers, FetchOptions } from '@pnpm/fetcher-base'
import type { AtomicResolution } from '@pnpm/resolver-base'
import type { CustomFetcher } from '@pnpm/hooks.types'

const f = fixtures(import.meta.dirname)

// Test helpers to reduce type casting
function createMockFetchers (partial: Partial<Fetchers> = {}): Fetchers {
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

function createMockCafs (partial: Partial<Cafs> = {}): Cafs {
  return {
    addFilesFromDir: jest.fn(),
    addFilesFromTarball: jest.fn() as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    ...partial,
  } as Cafs
}

function createMockResolution (resolution: Partial<AtomicResolution> & Record<string, any>): any { // eslint-disable-line @typescript-eslint/no-explicit-any
  return resolution
}

function createMockFetchOptions (opts: Partial<FetchOptions> = {}): any { // eslint-disable-line @typescript-eslint/no-explicit-any
  return opts
}

function createMockCustomFetcher (
  canFetch: CustomFetcher['canFetch'],
  fetch: CustomFetcher['fetch']
): CustomFetcher {
  return { canFetch, fetch }
}

/**
 * These tests demonstrate realistic custom fetcher implementations and verify
 * that the custom fetcher API works correctly for common use cases.
 */

describe('custom fetcher implementation examples', () => {
  describe('basic custom fetcher contract', () => {
    test('should successfully return FetchResult with manifest and filesIndex', async () => {
      const mockManifest = { name: 'test-package', version: '1.0.0' }
      const mockFilesIndex = { 'package.json': '/path/to/store/package.json' }

      const customFetcher = createMockCustomFetcher(
        () => true,
        async () => ({
          filesIndex: mockFilesIndex,
          manifest: mockManifest,
          requiresBuild: false,
        })
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'test-package@1.0.0' }
      )

      const result = await fetcher(
        createMockCafs(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        createMockFetchOptions()
      )

      expect(result.manifest).toEqual(mockManifest)
      expect(result.filesIndex).toEqual(mockFilesIndex)
      expect(result.requiresBuild).toBe(false)
    })

    test('should handle requiresBuild flag correctly', async () => {
      const customFetcher = createMockCustomFetcher(
        () => true,
        async () => ({
          filesIndex: {},
          manifest: { name: 'pkg', version: '1.0.0', scripts: { install: 'node install.js' } },
          requiresBuild: true,
        })
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      const result = await fetcher(
        createMockCafs(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        createMockFetchOptions()
      )

      expect(result.requiresBuild).toBe(true)
    })

    test('should propagate errors from custom fetcher', async () => {
      const customFetcher = createMockCustomFetcher(
        () => true,
        async () => {
          throw new Error('Network error during fetch')
        }
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      await expect(
        fetcher(
          createMockCafs(),
          createMockResolution({ tarball: 'http://example.com/package.tgz' }),
          createMockFetchOptions()
        )
      ).rejects.toThrow('Network error during fetch')
    })

    test('should pass CAFS to custom fetcher for file operations', async () => {
      let receivedCafs: Cafs | null = null

      const customFetcher = createMockCustomFetcher(
        () => true,
        async (cafs) => {
          receivedCafs = cafs
          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        }
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      const mockCafs = createMockCafs({ addFilesFromTarball: jest.fn() as any }) // eslint-disable-line @typescript-eslint/no-explicit-any
      await fetcher(
        mockCafs,
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        createMockFetchOptions()
      )

      expect(receivedCafs).toBe(mockCafs)
    })

    test('should pass progress callbacks to custom fetcher', async () => {
      const onStartFn = jest.fn()
      const onProgressFn = jest.fn()

      const customFetcher = createMockCustomFetcher(
        () => true,
        async (_cafs, _resolution, opts) => {
          // Custom fetcher can call progress callbacks
          opts.onStart?.(100, 1)
          ;(opts.onProgress as any)?.({ done: 50, total: 100 }) // eslint-disable-line @typescript-eslint/no-explicit-any

          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        }
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      await fetcher(
        createMockCafs(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        createMockFetchOptions({ onStart: onStartFn, onProgress: onProgressFn })
      )

      expect(onStartFn).toHaveBeenCalledWith(100, 1)
      expect(onProgressFn).toHaveBeenCalledWith({ done: 50, total: 100 })
    })

    test('should work with custom resolution types', async () => {
      const customResolution = createMockResolution({
        type: 'custom:cdn',
        cdnUrl: 'https://cdn.example.com/pkg.tgz',
      })

      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:cdn',
        async (_cafs, resolution) => {
          // Custom fetcher can access custom resolution fields
          expect(resolution.type).toBe('custom:cdn')
          expect((resolution as any).cdnUrl).toBe('https://cdn.example.com/pkg.tgz') // eslint-disable-line @typescript-eslint/no-explicit-any

          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        }
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        customResolution,
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      await fetcher(createMockCafs(), customResolution, createMockFetchOptions())
    })

    test('should allow custom fetcher.fetch to return partial manifest', async () => {
      const customFetcher = createMockCustomFetcher(
        () => true,
        async () => ({
          filesIndex: {},
          requiresBuild: false,
          // Manifest is optional in FetchResult
        })
      )

      const fetcher = await pickFetcher(
        createMockFetchers(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        { customFetchers: [customFetcher], packageId: 'pkg@1.0.0' }
      )

      const result = await fetcher(
        createMockCafs(),
        createMockResolution({ tarball: 'http://example.com/package.tgz' }),
        createMockFetchOptions()
      )

      expect(result.manifest).toBeUndefined()
      expect(result.filesIndex).toBeDefined()
    })
  })

  describe('delegating to tarball fetcher', () => {
    const registry = 'http://localhost:4873/'
    const tarballPath = f.find('babel-helper-hoist-variables-6.24.1.tgz')
    const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='

    test('custom fetcher can delegate to remoteTarball fetcher', async () => {
      const scope = nock(registry)
        .get('/custom-pkg.tgz')
        .replyWithFile(200, tarballPath, {
          'Content-Length': '1279',
        })

      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      // Create standard fetchers to pass to custom fetcher
      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {} }
      )

      // Custom fetcher that maps custom URLs to tarballs
      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:url' && Boolean((resolution as any).customUrl), // eslint-disable-line @typescript-eslint/no-explicit-any
        async (cafs, resolution, opts, fetchers) => {
          // Map custom resolution to tarball resolution
          const tarballResolution = {
            tarball: (resolution as any).customUrl, // eslint-disable-line @typescript-eslint/no-explicit-any
            integrity: tarballIntegrity,
          }

          // Delegate to standard tarball fetcher (passed via fetchers parameter)
          return fetchers.remoteTarball(cafs, tarballResolution, opts)
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:url',
        customUrl: `${registry}custom-pkg.tgz`,
      })

      const fetcher = await pickFetcher(
        tarballFetchers as Fetchers,
        customResolution,
        { customFetchers: [customFetcher], packageId: 'custom-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        customResolution,
        createMockFetchOptions({ filesIndexFile, lockfileDir: process.cwd() })
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
      expect(scope.isDone()).toBeTruthy()
    })

    test('custom fetcher can delegate to localTarball fetcher', async () => {
      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {} }
      )

      // Custom fetcher that maps custom local paths to tarballs
      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:local' && Boolean((resolution as any).localPath), // eslint-disable-line @typescript-eslint/no-explicit-any
        async (cafs, resolution, opts, fetchers) => {
          const tarballResolution = {
            tarball: `file:${(resolution as any).localPath}`, // eslint-disable-line @typescript-eslint/no-explicit-any
            integrity: tarballIntegrity,
          }

          return fetchers.localTarball(cafs, tarballResolution, opts)
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:local',
        localPath: tarballPath,
      })

      const fetcher = await pickFetcher(
        tarballFetchers as Fetchers,
        customResolution,
        { customFetchers: [customFetcher], packageId: 'local-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        customResolution,
        createMockFetchOptions({ filesIndexFile, lockfileDir: process.cwd() })
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
    })

    test('custom fetcher can transform resolution before delegating to tarball fetcher', async () => {
      const scope = nock(registry)
        .get('/transformed-pkg.tgz')
        .replyWithFile(200, tarballPath, {
          'Content-Length': '1279',
        })

      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {} }
      )

      // Custom fetcher that transforms custom resolution to tarball URL
      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:registry',
        async (cafs, resolution, opts, fetchers) => {
          // Transform custom registry format to standard tarball URL
          const tarballUrl = `${registry}${(resolution as any).packageName}.tgz` // eslint-disable-line @typescript-eslint/no-explicit-any

          const tarballResolution = {
            tarball: tarballUrl,
            integrity: tarballIntegrity,
          }

          return fetchers.remoteTarball(cafs, tarballResolution, opts)
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:registry',
        packageName: 'transformed-pkg',
      })

      const fetcher = await pickFetcher(
        tarballFetchers as Fetchers,
        customResolution,
        { customFetchers: [customFetcher], packageId: 'transformed-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        customResolution,
        createMockFetchOptions({ filesIndexFile, lockfileDir: process.cwd() })
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
      expect(scope.isDone()).toBeTruthy()
    })

    test('custom fetcher can use gitHostedTarball fetcher for custom git URLs', async () => {
      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {}, ignoreScripts: true }
      )

      // Custom fetcher that maps custom git resolution to git-hosted tarball
      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:git',
        async (cafs, resolution, opts, fetchers) => {
          // Map custom git resolution to GitHub codeload URL
          const tarballResolution = {
            tarball: `https://codeload.github.com/${(resolution as any).repo}/tar.gz/${(resolution as any).commit}`, // eslint-disable-line @typescript-eslint/no-explicit-any
          }

          return fetchers.gitHostedTarball(cafs, tarballResolution, opts)
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:git',
        repo: 'sveltejs/action-deploy-docs',
        commit: 'a65fbf5a90f53c9d72fed4daaca59da50f074355',
      })

      const fetcher = await pickFetcher(
        tarballFetchers as Fetchers,
        customResolution,
        { customFetchers: [customFetcher], packageId: 'git-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        customResolution,
        createMockFetchOptions({ filesIndexFile, lockfileDir: process.cwd() })
      )

      expect(result.filesIndex).toBeTruthy()
    })
  })

  describe('custom fetch implementations', () => {
    test('custom fetcher can implement custom caching logic', async () => {
      const fetchCalls: number[] = []
      const cache = new Map<string, any>() // eslint-disable-line @typescript-eslint/no-explicit-any

      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:cached',
        async (_cafs, resolution) => {
          fetchCalls.push(Date.now())

          // Check cache first
          const cacheKey = `${(resolution as any).url}@${(resolution as any).version}` // eslint-disable-line @typescript-eslint/no-explicit-any
          if (cache.has(cacheKey)) {
            return cache.get(cacheKey)
          }

          // Simulate fetch
          const result = {
            filesIndex: { 'package.json': '/store/pkg.json' },
            manifest: { name: 'cached-pkg', version: (resolution as any).version }, // eslint-disable-line @typescript-eslint/no-explicit-any
          }

          cache.set(cacheKey, result)
          return result
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:cached',
        url: 'https://cache.example.com/pkg',
        version: '1.0.0',
      })

      const fetcher = await pickFetcher(
        createMockFetchers(),
        customResolution,
        { customFetchers: [customFetcher], packageId: 'cached-pkg@1.0.0' }
      )

      // First fetch - should hit the fetch logic
      const result1 = await fetcher(createMockCafs(), customResolution, createMockFetchOptions())

      // Second fetch - should use cache
      const result2 = await fetcher(createMockCafs(), customResolution, createMockFetchOptions())

      expect(result1).toBe(result2)
      expect(fetchCalls).toHaveLength(2) // Fetcher called twice, but cache hit on second call
    })

    test('custom fetcher can implement authentication and token refresh', async () => {
      let authToken = 'initial-token'
      const authCalls: string[] = []

      const customFetcher = createMockCustomFetcher(
        (_pkgId, resolution) => resolution.type === 'custom:auth',
        async () => {
          authCalls.push(authToken)

          // Simulate token refresh on 401
          if (authToken === 'initial-token') {
            authToken = 'refreshed-token'
          }

          return {
            filesIndex: {},
            manifest: { name: 'auth-pkg', version: '1.0.0' },
            requiresBuild: false,
            authToken, // Could store for future use
          }
        }
      )

      const customResolution = createMockResolution({
        type: 'custom:auth',
        url: 'https://secure.example.com/pkg',
      })

      const fetcher = await pickFetcher(
        createMockFetchers(),
        customResolution,
        { customFetchers: [customFetcher], packageId: 'auth-pkg@1.0.0' }
      )

      const result = await fetcher(createMockCafs(), customResolution, createMockFetchOptions())

      expect(authCalls).toEqual(['initial-token'])
      expect((result as any).authToken).toBe('refreshed-token') // eslint-disable-line @typescript-eslint/no-explicit-any
    })
  })
})
