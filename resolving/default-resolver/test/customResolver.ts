/// <reference path="../../../__typings__/index.d.ts"/>
import { jest } from '@jest/globals'
import { createResolver } from '@pnpm/default-resolver'
import { type PackageDescriptor, type ResolverPlugin } from '@pnpm/hooks.types'
import { Response } from 'node-fetch'

test('custom resolver intercepts matching packages', async () => {
  const customResolver: ResolverPlugin = {
    supportsDescriptor: (descriptor: PackageDescriptor) => {
      return descriptor.name === 'test-package'
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    resolve: async (descriptor: PackageDescriptor, _opts: any) => {
      return {
        id: `custom:${descriptor.name}@${descriptor.range}`,
        resolution: {
          type: 'directory',
          directory: '/test/path',
        },
        getLockfileResolution: () => ({
          name: descriptor.name,
          version: descriptor.range,
        }),
        resolvedVia: 'custom-resolver',
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
  const customResolver: ResolverPlugin = {
    // Synchronous support check
    supportsDescriptor: (descriptor: PackageDescriptor) => {
      return descriptor.name.startsWith('@sync/')
    },
    // Synchronous resolution
    resolve: (descriptor: PackageDescriptor) => {
      return {
        id: `sync:${descriptor.name}@${descriptor.range}`,
        resolution: {
          tarball: `file://${descriptor.name}-${descriptor.range}.tgz`,
          integrity: 'sha512-test',
        },
        resolvedVia: 'sync-resolver',
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
  expect(result.resolvedVia).toBe('sync-resolver')
})

test('multiple resolvers - first matching wins', async () => {
  const resolver1: ResolverPlugin = {
    supportsDescriptor: (descriptor) => descriptor.name === 'shared-package',
    resolve: () => ({
      id: 'resolver-1:shared-package',
      resolution: { tarball: 'file://resolver1.tgz', integrity: 'sha512-1' },
      resolvedVia: 'resolver-1',
    }),
  }

  const resolver2: ResolverPlugin = {
    supportsDescriptor: (descriptor) => descriptor.name === 'shared-package',
    resolve: () => ({
      id: 'resolver-2:shared-package',
      resolution: { tarball: 'file://resolver2.tgz', integrity: 'sha512-2' },
      resolvedVia: 'resolver-2',
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

  // First resolver should win
  expect(result.id).toBe('resolver-1:shared-package')
  expect(result.resolvedVia).toBe('resolver-1')
})

test('getLockfileResolution transforms resolution', async () => {
  const getLockfileResolution = jest.fn((resolution: unknown) => ({ ...resolution as Record<string, unknown>, integrity: 'sha512-transformed' }))
  const customResolver: ResolverPlugin = {
    supportsDescriptor: (descriptor) => descriptor.name === 'transform-me',
    resolve: (descriptor) => ({
      id: 'transform-me@1.0.0',
      resolution: { tarball: 'file://transform-me.tgz', integrity: 'sha512-original' },
      resolvedVia: 'transform-resolver',
      getLockfileResolution,
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
    { alias: 'transform-me', bareSpecifier: '1.0.0' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  const lockfileResolution = result.getLockfileResolution?.(result.resolution)
  if (!lockfileResolution) {
    throw new Error('lockfileResolution is undefined')
  }
  expect((lockfileResolution as Record<string, unknown>).integrity).toBe('sha512-transformed')
})

test('custom resolver error handling', async () => {
  const customResolver: ResolverPlugin = {
    supportsDescriptor: () => true,
    resolve: () => {
      throw new Error('Resolver failed')
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

  await expect(resolve({ alias: 'any', bareSpecifier: '1.0.0' }, { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} })).rejects.toThrow('Resolver failed')
})

test('preferredVersions are passed to custom resolver', async () => {
  const resolve = jest.fn(() => ({
    id: 'test@1.0.0',
    resolution: { tarball: 'file://test.tgz', integrity: 'sha512-test' },
    resolvedVia: 'test-resolver',
  }))
  const customResolver: ResolverPlugin = {
    supportsDescriptor: () => true,
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

  expect(resolve).toHaveBeenCalledWith({ name: 'any', range: '1.0.0' }, { lockfileDir: '/test', projectDir: '/test', preferredVersions: { any: { '1.0.0': 'version' } } })
})

test('custom resolver can intercept any protocol', async () => {
  const customResolver: ResolverPlugin = {
    supportsDescriptor: (descriptor: PackageDescriptor) => {
      return descriptor.name.startsWith('custom-')
    },
    resolve: (descriptor: PackageDescriptor) => ({
      id: `custom-handled:${descriptor.name}@${descriptor.range}`,
      resolution: {
        type: 'directory',
        directory: `/custom/${descriptor.name}`,
      },
      resolvedVia: 'custom-resolver',
      manifest: {
        name: descriptor.name,
        version: '1.0.0',
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
  const customResolver: ResolverPlugin = {
    supportsDescriptor: (descriptor: PackageDescriptor) => {
      return descriptor.name.startsWith('custom-')
    },
    resolve: (descriptor: PackageDescriptor) => ({
      id: `custom:${descriptor.name}@${descriptor.range}`,
      resolution: { directory: '/custom' },
      resolvedVia: 'custom-resolver',
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
  const npmStyleResolver: ResolverPlugin = {
    supportsDescriptor: (descriptor) => {
      return !descriptor.range.includes(':')
    },
    resolve: (descriptor) => ({
      id: `custom-registry:${descriptor.name}@${descriptor.range}`,
      resolution: {
        tarball: `https://custom-registry.com/${descriptor.name}/-/${descriptor.name}-${descriptor.range}.tgz`,
        integrity: 'sha512-custom',
      },
      resolvedVia: 'custom-npm-resolver',
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

  expect(result.resolvedVia).toBe('custom-npm-resolver')
  expect('tarball' in result.resolution && result.resolution.tarball).toContain('custom-registry.com')
})