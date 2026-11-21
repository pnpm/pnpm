/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createResolver } from '@pnpm/resolving.default-resolver'

test('createResolver()', () => {
  const getAuthHeader = () => undefined
  const { resolve } = createResolver(createFetchFromRegistry({}), getAuthHeader, {
    cacheDir: '.cache',
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
  })
  expect(typeof resolve).toBe('function')
})

test('tarball preflight uses the resolver credentials and retry settings for HEAD and GET', async () => {
  const methods: string[] = []
  const { resolve } = createResolver(async (_url, opts) => {
    expect(opts?.authHeaderValue).toBe('Bearer fixture')
    expect(opts?.retry?.retries).toBe(0)
    methods.push(opts!.method!)
    return new Response(null, { status: opts?.method === 'HEAD' ? 405 : 200 })
  }, (_url, opts) => opts?.pkgName === '@sample/package' ? 'Bearer fixture' : undefined, {
    cacheDir: '.cache',
    registriesByScope: { default: 'https://registry.example/' },
    retry: { retries: 0 },
    storeDir: '.store',
  })
  const bareSpecifier = 'https://archive.example/package.tgz'
  const result = await resolve({ alias: '@sample/package', bareSpecifier }, {
    lockfileDir: '/test', projectDir: '/test', preferredVersions: {},
  })
  expect(result.resolution).toEqual({ tarball: bareSpecifier })
  expect(methods).toEqual(['HEAD', 'GET'])
})
