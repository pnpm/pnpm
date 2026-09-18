import fs from 'node:fs'
import path from 'node:path'

import { beforeAll, describe, expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
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
      // The update check resolves `pnpm@latest` through the same cache, which
      // would add a `pnpm.jsonl` entry to what these tests expect to find. It
      // is off under CI, so leaving it on would only fail locally.
      '--config.update-notifier=false',
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
      '--config.update-notifier=false',
    ])
  })
  test('list all metadata from the cache', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list'])

    expect(result).toBe(`http%3A+localhost+${REGISTRY_MOCK_PORT}/is-negative.jsonl
https%3A+registry.npmjs.org/is-negative.jsonl
https%3A+registry.npmjs.org/is-positive.jsonl`)
  })
  test('list all metadata from the cache related to the specified registry', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {
        registry: 'https://registry.npmjs.org/',
      },
      pnpmHomeDir: storeDir,
    }, ['list'])

    expect(result).toBe(`https%3A+registry.npmjs.org/is-negative.jsonl
https%3A+registry.npmjs.org/is-positive.jsonl`)
  })
  test('list all metadata from the cache that matches a pattern', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list', '*-positive'])

    expect(result).toBe('https%3A+registry.npmjs.org/is-positive.jsonl')
  })
  test('list registries as decoded URLs, matching cache view', async () => {
    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list-registries'])

    expect(result).toBe(`http://localhost:${REGISTRY_MOCK_PORT}/
https://registry.npmjs.org/`)
  })
  test('list registries skips stray files, as pnpm 12 does', async () => {
    // A registry is a directory of `.jsonl` files. macOS drops a `.DS_Store`
    // into any directory a user opens, which is not a registry.
    fs.writeFileSync(path.join(cacheDir, ABBREVIATED_META_DIR, '.DS_Store'), '')

    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: storeDir,
    }, ['list-registries'])

    expect(result).toBe(`http://localhost:${REGISTRY_MOCK_PORT}/
https://registry.npmjs.org/`)
  })
})
