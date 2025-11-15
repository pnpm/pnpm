import { pickFetcher } from '@pnpm/pick-fetcher'
import { jest } from '@jest/globals'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { fixtures } from '@pnpm/test-fixtures'
import { temporaryDirectory } from 'tempy'
import path from 'path'
import nock from 'nock'

const f = fixtures(import.meta.dirname)

/**
 * These tests demonstrate realistic adapter.fetch implementations and verify
 * that the adapter.fetch API works correctly for common use cases.
 */

describe('adapter.fetch implementation examples', () => {
  describe('basic adapter.fetch contract', () => {
    test('should successfully return FetchResult with manifest and filesIndex', async () => {
      const mockManifest = { name: 'test-package', version: '1.0.0' }
      const mockFilesIndex = { 'package.json': '/path/to/store/package.json' }

      const adapter = {
        canFetch: () => true,
        fetch: async (_cafs: any, _resolution: any, _opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          return {
            filesIndex: mockFilesIndex,
            manifest: mockManifest,
            requiresBuild: false,
          }
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const mockFetchers = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const fetcher = await pickFetcher(
        mockFetchers,
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'test-package@1.0.0' }
      )

      const mockCafs = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const result = await fetcher(mockCafs, { tarball: 'http://example.com/package.tgz' } as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any

      expect(result.manifest).toEqual(mockManifest)
      expect(result.filesIndex).toEqual(mockFilesIndex)
      expect(result.requiresBuild).toBe(false)
    })

    test('should handle requiresBuild flag correctly', async () => {
      const adapter = {
        canFetch: () => true,
        fetch: async () => ({
          filesIndex: {},
          manifest: { name: 'pkg', version: '1.0.0', scripts: { install: 'node install.js' } },
          requiresBuild: true,
        }),
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const mockFetchers = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const fetcher = await pickFetcher(
        mockFetchers,
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      const result = await fetcher({} as any, { tarball: 'http://example.com/package.tgz' } as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any

      expect(result.requiresBuild).toBe(true)
    })

    test('should propagate errors from adapter.fetch', async () => {
      const adapter = {
        canFetch: () => true,
        fetch: async () => {
          throw new Error('Network error during fetch')
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const mockFetchers = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const fetcher = await pickFetcher(
        mockFetchers,
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      await expect(
        fetcher({} as any, { tarball: 'http://example.com/package.tgz' } as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any
      ).rejects.toThrow('Network error during fetch')
    })

    test('should pass CAFS to adapter.fetch for file operations', async () => {
      let receivedCafs: any = null // eslint-disable-line @typescript-eslint/no-explicit-any

      const adapter = {
        canFetch: () => true,
        fetch: async (cafs: any, _resolution: any, _opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          receivedCafs = cafs
          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const mockFetchers = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const fetcher = await pickFetcher(
        mockFetchers,
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      const mockCafs = { addFilesFromTarball: jest.fn() }
      await fetcher(mockCafs as any, { tarball: 'http://example.com/package.tgz' } as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any

      expect(receivedCafs).toBe(mockCafs)
    })

    test('should pass progress callbacks to adapter.fetch', async () => {
      const onStartFn = jest.fn()
      const onProgressFn = jest.fn()

      const adapter = {
        canFetch: () => true,
        fetch: async (_cafs: any, _resolution: any, opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Adapter can call progress callbacks
          opts.onStart?.(100, 1)
          opts.onProgress?.({ done: 50, total: 100 })

          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const mockFetchers = {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      const fetcher = await pickFetcher(
        mockFetchers,
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      await fetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { onStart: onStartFn, onProgress: onProgressFn } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(onStartFn).toHaveBeenCalledWith(100, 1)
      expect(onProgressFn).toHaveBeenCalledWith({ done: 50, total: 100 })
    })

    test('should work with custom resolution types', async () => {
      const customResolution = { type: '@company/cdn', cdnUrl: 'https://cdn.example.com/pkg.tgz' }

      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => resolution.type === '@company/cdn', // eslint-disable-line @typescript-eslint/no-explicit-any
        fetch: async (_cafs: any, resolution: any, _opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Adapter can access custom resolution fields
          expect(resolution.type).toBe('@company/cdn')
          expect(resolution.cdnUrl).toBe('https://cdn.example.com/pkg.tgz')

          return {
            filesIndex: {},
            manifest: { name: 'pkg', version: '1.0.0' },
            requiresBuild: false,
          }
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        customResolution as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      await fetcher({} as any, customResolution as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    })

    test('should allow adapter.fetch to return partial manifest', async () => {
      const adapter = {
        canFetch: () => true,
        fetch: async () => ({
          filesIndex: {},
          requiresBuild: false,
          // Manifest is optional in FetchResult
        }),
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { tarball: 'http://example.com/package.tgz' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'pkg@1.0.0' }
      )

      const result = await fetcher({} as any, { tarball: 'http://example.com/package.tgz' } as any, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any

      expect(result.manifest).toBeUndefined()
      expect(result.filesIndex).toBeDefined()
    })
  })

  describe('delegating to tarball fetcher', () => {
    const registry = 'http://localhost:4873/'
    const tarballPath = f.find('babel-helper-hoist-variables-6.24.1.tgz')
    const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='

    test('adapter can delegate to remoteTarball fetcher', async () => {
      const scope = nock(registry)
        .get('/custom-pkg.tgz')
        .replyWithFile(200, tarballPath, {
          'Content-Length': '1279',
        })

      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      // Create standard fetchers to pass to adapter
      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {} }
      )

      // Adapter that maps custom URLs to tarballs
      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          return resolution.type === '@company/custom' && Boolean(resolution.customUrl)
        },
        fetch: async (cafs: any, resolution: any, opts: any, fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Map custom resolution to tarball resolution
          const tarballResolution = {
            tarball: resolution.customUrl,
            integrity: tarballIntegrity,
          }

          // Delegate to standard tarball fetcher (passed via fetchers parameter)
          return fetchers.remoteTarball(cafs, tarballResolution, opts)
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        tarballFetchers as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/custom', customUrl: `${registry}custom-pkg.tgz` } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'custom-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        { type: '@company/custom', customUrl: `${registry}custom-pkg.tgz` } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { filesIndexFile, lockfileDir: process.cwd() } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
      expect(scope.isDone()).toBeTruthy()
    })

    test('adapter can delegate to localTarball fetcher', async () => {
      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {} }
      )

      // Adapter that maps custom local paths to tarballs
      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          return resolution.type === '@company/local' && Boolean(resolution.localPath)
        },
        fetch: async (cafs: any, resolution: any, opts: any, fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          const tarballResolution = {
            tarball: `file:${resolution.localPath}`,
            integrity: tarballIntegrity,
          }

          return fetchers.localTarball(cafs, tarballResolution, opts)
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        tarballFetchers as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/local', localPath: tarballPath } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'local-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        { type: '@company/local', localPath: tarballPath } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { filesIndexFile, lockfileDir: process.cwd() } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
    })

    test('adapter can transform resolution before delegating to tarball fetcher', async () => {
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

      // Adapter that transforms custom resolution to tarball URL
      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          return resolution.type === '@company/registry'
        },
        fetch: async (cafs: any, resolution: any, opts: any, fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Transform custom registry format to standard tarball URL
          const tarballUrl = `${registry}${resolution.packageName}.tgz`

          const tarballResolution = {
            tarball: tarballUrl,
            integrity: tarballIntegrity,
          }

          return fetchers.remoteTarball(cafs, tarballResolution, opts)
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        tarballFetchers as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/registry', packageName: 'transformed-pkg' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'transformed-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        { type: '@company/registry', packageName: 'transformed-pkg' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { filesIndexFile, lockfileDir: process.cwd() } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(result.filesIndex['package.json']).toBeTruthy()
      expect(scope.isDone()).toBeTruthy()
    })

    test('adapter can use gitHostedTarball fetcher for custom git URLs', async () => {
      const storeDir = temporaryDirectory()
      const cafs = createCafsStore(storeDir)
      const filesIndexFile = path.join(storeDir, 'index.json')

      const fetchFromRegistry = createFetchFromRegistry({})
      const tarballFetchers = createTarballFetcher(
        fetchFromRegistry,
        () => undefined,
        { rawConfig: {}, ignoreScripts: true }
      )

      // Adapter that maps custom git resolution to git-hosted tarball
      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          return resolution.type === '@company/git'
        },
        fetch: async (cafs: any, resolution: any, opts: any, fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Map custom git resolution to GitHub codeload URL
          const tarballResolution = {
            tarball: `https://codeload.github.com/${resolution.repo}/tar.gz/${resolution.commit}`,
          }

          return fetchers.gitHostedTarball(cafs, tarballResolution, opts)
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        tarballFetchers as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        {
          type: '@company/git',
          repo: 'sveltejs/action-deploy-docs',
          commit: 'a65fbf5a90f53c9d72fed4daaca59da50f074355',
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'git-pkg@1.0.0' }
      )

      const result = await fetcher(
        cafs,
        {
          type: '@company/git',
          repo: 'sveltejs/action-deploy-docs',
          commit: 'a65fbf5a90f53c9d72fed4daaca59da50f074355',
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { filesIndexFile, lockfileDir: process.cwd() } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(result.filesIndex).toBeTruthy()
    })
  })

  describe('custom fetch implementations', () => {
    test('adapter can implement custom caching logic', async () => {
      const fetchCalls: number[] = []
      const cache = new Map<string, any>() // eslint-disable-line @typescript-eslint/no-explicit-any

      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => resolution.type === '@company/cached', // eslint-disable-line @typescript-eslint/no-explicit-any
        fetch: async (_cafs: any, resolution: any, _opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          fetchCalls.push(Date.now())

          // Check cache first
          const cacheKey = `${resolution.url}@${resolution.version}`
          if (cache.has(cacheKey)) {
            return cache.get(cacheKey)
          }

          // Simulate fetch
          const result = {
            filesIndex: { 'package.json': '/store/pkg.json' },
            manifest: { name: 'cached-pkg', version: resolution.version },
          }

          cache.set(cacheKey, result)
          return result
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/cached', url: 'https://cache.example.com/pkg', version: '1.0.0' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'cached-pkg@1.0.0' }
      )

      // First fetch - should hit the fetch logic
      const result1 = await fetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/cached', url: 'https://cache.example.com/pkg', version: '1.0.0' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      // Second fetch - should use cache
      const result2 = await fetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/cached', url: 'https://cache.example.com/pkg', version: '1.0.0' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(result1).toBe(result2)
      expect(fetchCalls).toHaveLength(2) // Fetcher called twice, but cache hit on second call
    })

    test('adapter can implement authentication and token refresh', async () => {
      let authToken = 'initial-token'
      const authCalls: string[] = []

      const adapter = {
        canFetch: (_pkgId: any, resolution: any) => resolution.type === '@company/auth', // eslint-disable-line @typescript-eslint/no-explicit-any
        fetch: async (_cafs: any, resolution: any, _opts: any, _fetchers: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          authCalls.push(authToken)

          // Simulate token refresh on 401
          if (authToken === 'initial-token') {
            authToken = 'refreshed-token'
          }

          return {
            filesIndex: {},
            manifest: { name: 'auth-pkg', version: '1.0.0' },
            authToken, // Could store for future use
          }
        },
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any

      const fetcher = await pickFetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/auth', url: 'https://secure.example.com/pkg' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { adapters: [adapter], packageId: 'auth-pkg@1.0.0' }
      )

      const result = await fetcher(
        {} as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        { type: '@company/auth', url: 'https://secure.example.com/pkg' } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )

      expect(authCalls).toEqual(['initial-token'])
      expect((result as any).authToken).toBe('refreshed-token') // eslint-disable-line @typescript-eslint/no-explicit-any
    })
  })
})
