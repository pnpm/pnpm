import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

describe('cache view', () => {
  let cacheDir: string
  let storeDir: string
  beforeEach(async () => {
    prepare()
    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')

    await execa('node', [
      pnpmBin,
      'add',
      'is-negative@2.1.0',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      '--config.resolution-mode=highest',
      `--registry=${REGISTRY}`,
    ])
    rimrafSync('node_modules')
    rimrafSync('pnpm-lock.yaml')
    await execa('node', [
      pnpmBin,
      'add',
      'is-negative@2.1.0',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      '--config.resolution-mode=highest',
    ])
  })
  test('lists all metadata for requested package', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: process.cwd(),
      storeDir,
    }, ['view', 'is-negative'])

    expect(JSON.parse(result!)).toMatchObject({
      [`localhost:${REGISTRY_MOCK_PORT}`]: {
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      },
      'registry.npmjs.org': {
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      },
    })
  })
  test('lists metadata for requested package from specified registry', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {
        registry: 'https://registry.npmjs.org/',
      },
      pnpmHomeDir: process.cwd(),
      storeDir,
    }, ['view', 'is-negative'])

    expect(JSON.parse(result!)).toMatchObject({
      'registry.npmjs.org': {
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      },
    })
  })

  test('lists all metadata for requested package should specify a package name', async () => {
    await expect(
      cache.handler({
        cacheDir,
        cliOptions: {},
        pnpmHomeDir: process.cwd(),
        storeDir,
      }, ['view'])
    ).rejects.toThrow('`pnpm cache view` requires the package name')
  })

  test('lists all metadata for requested package should not accept more than one package name', async () => {
    await expect(
      cache.handler({
        cacheDir,
        cliOptions: {},
        pnpmHomeDir: process.cwd(),
        storeDir,
      }, ['view', 'is-negative', 'is-positive'])
    ).rejects.toThrow('`pnpm cache view` only accepts one package name')
  })
})
