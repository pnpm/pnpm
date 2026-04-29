import path from 'node:path'

import { beforeAll, describe, expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

describe('cache', () => {
  let cacheDir: string
  let storeDir: string
  beforeAll(async () => {
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
      'is-positive@1.0.0',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      '--config.resolution-mode=highest',
    ])
  })
  test('list all metadata from the cache', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list'])

    expect(result).toBe(`localhost+${REGISTRY_MOCK_PORT}/is-negative.jsonl
registry.npmjs.org/is-negative.jsonl
registry.npmjs.org/is-positive.jsonl`)
  })
  test('list all metadata from the cache related to the specified registry', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {
        registry: 'https://registry.npmjs.org/',
      },
      pnpmHomeDir: storeDir,
    }, ['list'])

    expect(result).toBe(`registry.npmjs.org/is-negative.jsonl
registry.npmjs.org/is-positive.jsonl`)
  })
  test('list all metadata from the cache that matches a pattern', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list', '*-positive'])

    expect(result).toBe('registry.npmjs.org/is-positive.jsonl')
  })
  test('list registries', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list-registries'])

    expect(result).toBe(`localhost+${REGISTRY_MOCK_PORT}
registry.npmjs.org`)
  })
})
