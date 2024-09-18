import path from 'path'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa from 'execa'
import { cache } from '@pnpm/cache.commands'
import { sync as rimraf } from '@zkochan/rimraf'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')
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
    rimraf('node_modules')
    rimraf('pnpm-lock.yaml')
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

    expect(JSON.parse(result!)).toEqual(expect.objectContaining({
      [`localhost:${REGISTRY_MOCK_PORT}`]: expect.objectContaining({
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      }),
      'registry.npmjs.org': expect.objectContaining({
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      }),
    }))
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

    expect(JSON.parse(result!)).toEqual(expect.objectContaining({
      'registry.npmjs.org': expect.objectContaining({
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      }),
    }))
  })
})
