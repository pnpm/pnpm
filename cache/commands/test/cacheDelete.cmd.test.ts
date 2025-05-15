import path from 'path'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa from 'execa'
import { cache } from '@pnpm/cache.commands'
import { sync as rimraf } from '@zkochan/rimraf'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

describe('cache delete', () => {
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
      'is-positive@1.0.0',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      '--config.resolution-mode=highest',
    ])
  })
  test('delete all metadata from the cache that matches a pattern', async () => {
    await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['delete', '*-positive'])
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list'])

    expect(result).toEqual(`localhost+${REGISTRY_MOCK_PORT}/is-negative.json
registry.npmjs.org/is-negative.json`)
  })
})
