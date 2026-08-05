import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'
import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
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

    expect(result).toBe(`localhost+${REGISTRY_MOCK_PORT}/is-negative.jsonl
registry.npmjs.org/is-negative.jsonl`)
  })
})

describe('cache delete across metadata directories', () => {
  test('deletes a package from every metadata cache directory, not only the one the current mode reads', async () => {
    prepare()
    const cacheDir = path.resolve('cache')
    const registryName = 'registry.npmjs.org'
    const metaDirs = [ABBREVIATED_META_DIR, FULL_META_DIR, FULL_FILTERED_META_DIR]
    const sentinel = (metaDir: string) => path.join(cacheDir, metaDir, registryName, '@vue', 'compiler-core.jsonl')
    for (const metaDir of metaDirs) {
      fs.mkdirSync(path.dirname(sentinel(metaDir)), { recursive: true })
      fs.writeFileSync(sentinel(metaDir), '')
    }

    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: cacheDir,
    }, ['delete', '@vue/compiler-core'])

    for (const metaDir of metaDirs) {
      expect(fs.existsSync(sentinel(metaDir))).toBe(false)
    }
    expect(result).toBe(`${registryName}/@vue/compiler-core.jsonl`)
  })

  test('deletes metadata cached under a non-current mode even when the other directories are absent', async () => {
    prepare()
    const cacheDir = path.resolve('cache')
    // The default mode reads `metadata`, but the package is only cached under
    // `metadata-full-filtered` and the other directories don't exist.
    const sentinel = path.join(cacheDir, FULL_FILTERED_META_DIR, 'registry.npmjs.org', '@vue', 'compiler-core.jsonl')
    fs.mkdirSync(path.dirname(sentinel), { recursive: true })
    fs.writeFileSync(sentinel, '')

    await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: cacheDir,
    }, ['delete', '@vue/compiler-core'])

    expect(fs.existsSync(sentinel)).toBe(false)
  })
})
