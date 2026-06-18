/// <reference path="../../../__typings__/index.d.ts"/>
import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const registries = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
} satisfies Registries

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

const DARWIN_INTEGRITY = 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=='
const LINUX_INTEGRITY = 'sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=='

function wrapperMeta () {
  return {
    name: 'pacquet',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'pacquet',
        version: '1.0.0',
        bin: { pacquet: 'bin/pacquet' },
        optionalDependencies: {
          'pacquet-darwin-arm64': '1.0.0',
          'pacquet-linux-x64': '1.0.0',
        } as Record<string, string>,
        dist: {
          integrity: 'sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==',
          shasum: '0000000000000000000000000000000000000000',
          tarball: 'https://registry.npmjs.org/pacquet/-/pacquet-1.0.0.tgz',
        },
      },
    },
  }
}

function platformMeta (name: string, os: string, cpu: string, integrity: string) {
  return {
    name,
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        os: [os],
        cpu: [cpu],
        dist: {
          integrity,
          shasum: '1111111111111111111111111111111111111111',
          tarball: `https://registry.npmjs.org/${name}/-/${name}-1.0.0.tgz`,
        },
      },
    },
  }
}

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('a native bin dependency resolves to a variations resolution over its platform packages', async () => {
  const pool = getMockAgent().get(registries.default.replace(/\/$/, ''))
  pool.intercept({ path: '/pacquet', method: 'GET' }).reply(200, wrapperMeta())
  pool.intercept({ path: '/pacquet-darwin-arm64', method: 'GET' })
    .reply(200, platformMeta('pacquet-darwin-arm64', 'darwin', 'arm64', DARWIN_INTEGRITY))
  pool.intercept({ path: '/pacquet-linux-x64', method: 'GET' })
    .reply(200, platformMeta('pacquet-linux-x64', 'linux', 'x64', LINUX_INTEGRITY))

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'pacquet', bareSpecifier: '1.0.0' }, {})

  expect(resolveResult!.id).toBe('pacquet@1.0.0')
  expect(resolveResult!.resolution).toStrictEqual({
    type: 'variations',
    variants: [
      {
        resolution: {
          type: 'binary',
          archive: 'tarball',
          url: 'https://registry.npmjs.org/pacquet-darwin-arm64/-/pacquet-darwin-arm64-1.0.0.tgz',
          integrity: DARWIN_INTEGRITY,
          bin: { pacquet: 'pacquet' },
        },
        targets: [{ os: 'darwin', cpu: 'arm64' }],
      },
      {
        resolution: {
          type: 'binary',
          archive: 'tarball',
          url: 'https://registry.npmjs.org/pacquet-linux-x64/-/pacquet-linux-x64-1.0.0.tgz',
          integrity: LINUX_INTEGRITY,
          bin: { pacquet: 'pacquet' },
        },
        targets: [{ os: 'linux', cpu: 'x64' }],
      },
    ],
  })
  // The launcher shim's bin path is replaced by the native binary at the package root.
  expect(resolveResult!.manifest.bin).toStrictEqual({ pacquet: process.platform === 'win32' ? 'pacquet.exe' : 'pacquet' })
})

test('a native bin dependency with no platform optional deps falls back to a tarball resolution', async () => {
  const meta = wrapperMeta()
  meta.versions['1.0.0'].optionalDependencies = {}
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pacquet', method: 'GET' }).reply(200, meta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'pacquet', bareSpecifier: '1.0.0' }, {})

  expect(resolveResult!.resolution).toStrictEqual({
    integrity: meta.versions['1.0.0'].dist.integrity,
    tarball: 'https://registry.npmjs.org/pacquet/-/pacquet-1.0.0.tgz',
  })
})
