import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
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
      [`http://localhost:${REGISTRY_MOCK_PORT}/`]: {
        cachedVersions: ['2.1.0'],
        nonCachedVersions: [
          '1.0.0',
          '1.0.1',
          '2.0.0',
          '2.0.1',
          '2.0.2',
        ],
      },
      'https://registry.npmjs.org/': {
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
      'https://registry.npmjs.org/': {
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

  // A damaged file is what the resolver refuses to read, so listing the
  // versions around the damage would report a cache no install will use.
  test('omits a package whose cache file is damaged', async () => {
    const mirror = path.join(cacheDir, ABBREVIATED_META_DIR, 'https%3A+registry.npmjs.org', 'is-negative.jsonl')
    const bytes = fs.readFileSync(mirror)
    const at = bytes.indexOf(Buffer.from('"integrity"'))
    expect(at).toBeGreaterThan(-1)
    // Same byte count, so the index spans still address the fragment, but
    // its own bytes no longer parse.
    bytes.fill('x', at, at + 11)
    fs.writeFileSync(mirror, bytes)

    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: process.cwd(),
      storeDir,
    }, ['view', 'is-negative'])

    expect(JSON.parse(result!)['https://registry.npmjs.org/']).toBeUndefined()
  })

  // The registry served that version in the wrong shape, which the resolver
  // reads as a version without a manifest rather than as a damaged file.
  test('skips a version whose cached manifest has the wrong shape', async () => {
    const mirror = path.join(cacheDir, ABBREVIATED_META_DIR, 'https%3A+registry.npmjs.org', 'is-negative.jsonl')
    const bytes = fs.readFileSync(mirror)
    const lineEnd = bytes.indexOf(10)
    const [, headersLen, indexLen] = bytes.toString('utf8', 0, lineEnd).split(' ').map(Number)
    const indexStart = lineEnd + 1 + headersLen
    const index = JSON.parse(bytes.toString('utf8', indexStart, indexStart + indexLen)) as { versions: Array<[string, number, number]> }
    const [version, offset, length] = index.versions.find(([candidate]) => candidate === '2.0.2')!
    bytes.write(`[${' '.repeat(length - 2)}]`, indexStart + indexLen + offset)
    fs.writeFileSync(mirror, bytes)

    const result = await cache.handler({
      cacheDir,
      cliOptions: {},
      pnpmHomeDir: process.cwd(),
      storeDir,
    }, ['view', 'is-negative'])

    const view = JSON.parse(result!)['https://registry.npmjs.org/']
    expect(view.cachedVersions).not.toContain(version)
    expect(view.nonCachedVersions).not.toContain(version)
    expect(view.cachedVersions.length + view.nonCachedVersions.length).toBeGreaterThan(0)
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
