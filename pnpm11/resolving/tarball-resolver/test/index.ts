/// <reference path="../../../__typings__/index.d.ts"/>
// cspell:ignore buildserver
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { resolveFromTarball as _resolveFromTarball, storeTarballResolution } from '@pnpm/resolving.tarball-resolver'

const fetch = createFetchFromRegistry({})
const resolveFromTarball = _resolveFromTarball.bind(null, fetch)

test('tarball from npm registry (immutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedBareSpecifier: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})
test('tarball from npm.jsr.io registry (immutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    normalizedBareSpecifier: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    resolution: {
      tarball: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL that contain port number', async () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fetch: any = async (url: string) => ({ url })
  const resolutionResult = await _resolveFromTarball(fetch, { bareSpecifier: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    normalizedBareSpecifier: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    resolution: {
      tarball: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL with redundant port', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://registry.npmjs.org:443/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedBareSpecifier: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL that redirects to a different URL (immutable)', async () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fetch: any = async (url: string) => {
    if (url === 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz') {
      return {
        url: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
        headers: new Map([['cache-control', 'immutable']]),
      }
    }
    return { url }
  }
  const resolutionResult = await _resolveFromTarball(fetch, { bareSpecifier: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedBareSpecifier: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball not from npm registry (mutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://github.com/hegemonic/taffydb/tarball/master',
    normalizedBareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master',
    resolution: {
      tarball: 'https://github.com/hegemonic/taffydb/tarball/master',
    },
    resolvedVia: 'url',
  })
})

test('a fresh Cache-Control record resolves an https tarball without a request', async () => {
  const cacheDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-tarball-cache-'))
  const url = 'https://example.com/pkg-from-tarball-1.0.0.tgz'
  storeTarballResolution(cacheDir, {
    url,
    tarball: url,
    integrity: 'sha512-abc',
    etag: '"pkg-from-tarball"',
    cacheControl: 'public, max-age=31536000, immutable',
    fetchedAt: Date.now(),
  })
  const fetch = async () => {
    throw new Error('a fresh tarball must not be requested')
  }
  const resolutionResult = await _resolveFromTarball(fetch, { bareSpecifier: url }, { cacheDir })
  expect(resolutionResult?.resolution).toStrictEqual({
    tarball: url,
    integrity: 'sha512-abc',
  })
  fs.rmSync(cacheDir, { recursive: true, force: true })
})

test('tarballs from GitHub (is-negative)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    normalizedBareSpecifier: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    resolution: {
      tarball: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    },
    resolvedVia: 'url',
  })
})
