/// <reference path="../../../__typings__/index.d.ts"/>
import { jest } from '@jest/globals'
import { createResolver } from '@pnpm/default-resolver'
import { type WantedDependency, type CustomResolver } from '@pnpm/hooks.types'
import { Response } from 'node-fetch'

test('custom resolver intercepts matching packages', async () => {
  const customResolver: CustomResolver = {
    canResolve: (wantedDependency: WantedDependency) => {
      return wantedDependency.alias === 'test-package'
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    resolve: async (wantedDependency: WantedDependency, _opts: any) => {
      return {
        id: `custom:${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
        resolution: {
          type: 'directory',
          directory: '/test/path',
        },
      }
    },
  }

  const fetchFromRegistry = async (): Promise<Response> => new Response('')
  const getAuthHeader = () => undefined

  const { resolve } = createResolver(fetchFromRegistry, getAuthHeader, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'test-package', bareSpecifier: '1.0.0' },
    {
      lockfileDir: '/test',
      projectDir: '/test',
      preferredVersions: {},
    }
  )

  expect(result.id).toBe('custom:test-package@1.0.0')
  expect(result.resolvedVia).toBe('custom-resolver')
})

test('custom resolver with synchronous methods', async () => {
  const customResolver: CustomResolver = {
    // Synchronous support check
    canResolve: (wantedDependency: WantedDependency) => {
      return wantedDependency.alias!.startsWith('@sync/')
    },
    // Synchronous resolution
    resolve: (wantedDependency: WantedDependency) => {
      return {
        id: `sync:${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
        resolution: {
          tarball: `file://${wantedDependency.alias}-${wantedDependency.bareSpecifier}.tgz`,
          integrity: 'sha512-test',
        },
      }
    },
  }

  const fetchFromRegistry = async (): Promise<Response> => new Response('')
  const getAuthHeader = () => undefined

  const { resolve } = createResolver(fetchFromRegistry, getAuthHeader, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: '@sync/test', bareSpecifier: '2.0.0' },
    {
      lockfileDir: '/test',
      projectDir: '/test',
      preferredVersions: {},
    }
  )

  expect(result.id).toBe('sync:@sync/test@2.0.0')
  expect(result.resolvedVia).toBe('custom-resolver')
})

test('multiple custom resolvers - first matching wins', async () => {
  const resolver1: CustomResolver = {
    canResolve: (wantedDependency) => wantedDependency.alias === 'shared-package',
    resolve: () => ({
      id: 'resolver-1:shared-package',
      resolution: { tarball: 'file://resolver1.tgz', integrity: 'sha512-1' },
    }),
  }

  const resolver2: CustomResolver = {
    canResolve: (wantedDependency) => wantedDependency.alias === 'shared-package',
    resolve: () => ({
      id: 'resolver-2:shared-package',
      resolution: { tarball: 'file://resolver2.tgz', integrity: 'sha512-2' },
    }),
  }

  const fetchFromRegistry = async (): Promise<Response> => new Response('')
  const getAuthHeader = () => undefined

  const { resolve } = createResolver(fetchFromRegistry, getAuthHeader, {
    customResolvers: [resolver1, resolver2], // Order matters
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'shared-package', bareSpecifier: '1.0.0' },
    {
      lockfileDir: '/test',
      projectDir: '/test',
      preferredVersions: {},
    }
  )

  // First custom resolver should win
  expect(result.id).toBe('resolver-1:shared-package')
  expect(result.resolvedVia).toBe('custom-resolver')
})

test('custom resolver error handling', async () => {
  const customResolver: CustomResolver = {
    canResolve: () => true,
    resolve: () => {
      throw new Error('Custom resolver failed')
    },
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  await expect(resolve({ alias: 'any', bareSpecifier: '1.0.0' }, { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} })).rejects.toThrow('Custom resolver failed')
})

test('preferredVersions are passed to custom resolver', async () => {
  const resolve = jest.fn(() => ({
    id: 'test@1.0.0',
    resolution: { tarball: 'file://test.tgz', integrity: 'sha512-test' },
  }))
  const customResolver: CustomResolver = {
    canResolve: () => true,
    resolve,
  }

  const { resolve: resolvePackage } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  await resolvePackage(
    { alias: 'any', bareSpecifier: '1.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: { any: { '1.0.0': 'version' } } as unknown as Record<string, Record<string, 'version' | 'range' | 'tag'>> }
  )

  expect(resolve).toHaveBeenCalledWith({ alias: 'any', bareSpecifier: '1.0.0' }, { lockfileDir: '/test', projectDir: '/test', preferredVersions: { any: { '1.0.0': 'version' } } })
})

test('custom resolver can intercept any protocol', async () => {
  const customResolver: CustomResolver = {
    canResolve: (wantedDependency: WantedDependency) => {
      return wantedDependency.alias!.startsWith('custom-')
    },
    resolve: (wantedDependency: WantedDependency) => ({
      id: `custom-handled:${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
      resolution: {
        type: '@test/custom',
        directory: `/custom/${wantedDependency.alias}`,
      },
    }),
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'custom-package', bareSpecifier: 'file:../some-path' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('custom-resolver')
  expect(result.id).toBe('custom-handled:custom-package@file:../some-path')
})

test('custom resolver falls through when not supported', async () => {
  const customResolver: CustomResolver = {
    canResolve: (wantedDependency: WantedDependency) => {
      return wantedDependency.alias!.startsWith('custom-')
    },
    resolve: (wantedDependency: WantedDependency) => ({
      id: `custom:${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
      resolution: { type: '@test/custom', directory: '/custom' },
    }),
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [customResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  await expect(
    resolve(
      { alias: 'regular-package', bareSpecifier: 'file:../nonexistent' },
      { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
    )
  ).rejects.toThrow()
})

test('custom resolver can override npm registry resolution', async () => {
  const npmStyleResolver: CustomResolver = {
    canResolve: (wantedDependency) => {
      return !wantedDependency.bareSpecifier!.includes(':')
    },
    resolve: (wantedDependency) => ({
      id: `custom-registry:${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
      resolution: {
        tarball: `https://custom-registry.com/${wantedDependency.alias}/-/${wantedDependency.alias}-${wantedDependency.bareSpecifier}.tgz`,
        integrity: 'sha512-custom',
      },
    }),
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [npmStyleResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'express', bareSpecifier: '^4.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('custom-resolver')
  expect('tarball' in result.resolution && result.resolution.tarball).toContain('custom-registry.com')
})

// Fetch phase custom fetcher tests - showing complete fetcher replacements

test('custom custom fetcher: reuse local tarball fetcher', async () => {
  // This demonstrates how a custom resolver can reuse pnpm's local tarball fetcher
  // for a custom protocol like "company-local:package-name"
  const localTarballResolver: CustomResolver = {
    canResolve: (wantedDependency) => wantedDependency.alias!.startsWith('company-local:'),
    resolve: (wantedDependency) => {
      const actualName = wantedDependency.alias!.replace('company-local:', '')
      return {
        id: wantedDependency.alias!,
        resolution: {
          type: '@company/local',
          localPath: `/company/tarballs/${actualName}-${wantedDependency.bareSpecifier}.tgz`,
        },
      }
    },
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [localTarballResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'company-local:my-package', bareSpecifier: '1.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('custom-resolver')
  expect(result.resolution).toHaveProperty('type', '@company/local')
})

test('custom custom fetcher: reuse remote tarball downloader', async () => {
  // This demonstrates fetching from a custom CDN using pnpm's download utilities
  // for a custom protocol like "cdn:package-name"
  const cdnResolver: CustomResolver = {
    canResolve: (wantedDependency) => wantedDependency.alias!.startsWith('cdn:'),
    resolve: (wantedDependency) => {
      const actualName = wantedDependency.alias!.replace('cdn:', '')
      return {
        id: wantedDependency.alias!,
        resolution: {
          type: '@company/cdn',
          cdnUrl: `https://cdn.example.com/packages/${actualName}/${wantedDependency.bareSpecifier}/${actualName}-${wantedDependency.bareSpecifier}.tgz`,
        },
      }
    },
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [cdnResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'cdn:awesome-lib', bareSpecifier: '2.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('custom-resolver')
  expect(result.resolution).toHaveProperty('type', '@company/cdn')
})

test('custom custom fetcher: wrap npm registry with custom logic', async () => {
  // This demonstrates wrapping/enhancing standard npm registry resolution and fetching
  // for a protocol like "private-npm:package-name" that uses private registry
  const privateNpmResolver: CustomResolver = {
    canResolve: (wantedDependency) => wantedDependency.alias!.startsWith('private-npm:'),
    resolve: async (wantedDependency, opts) => {
      const actualName = wantedDependency.alias!.replace('private-npm:', '')

      // In a real implementation, you'd fetch from your private registry here
      // For this test, we mock the registry response
      return {
        id: `private-npm:${actualName}@${wantedDependency.bareSpecifier}`,
        resolution: {
          tarball: `https://private-registry.company.com/${actualName}/-/${actualName}-${wantedDependency.bareSpecifier}.tgz`,
          integrity: 'sha512-mock-integrity',
          registry: 'https://private-registry.company.com/',
        },
      }
    },
  }

  const { resolve } = createResolver(async () => new Response(''), () => undefined, {
    customResolvers: [privateNpmResolver],
    rawConfig: {},
    cacheDir: '/tmp/test-cache',
    offline: false,
    preferOffline: false,
    retry: {},
    timeout: 60000,
    registries: { default: 'https://registry.npmjs.org/' },
  })

  const result = await resolve(
    { alias: 'private-npm:company-utils', bareSpecifier: '3.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('custom-resolver')
  expect('tarball' in result.resolution && result.resolution.tarball).toContain('private-registry.company.com')
})