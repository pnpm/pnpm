import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import {
  fetchShasumsFileCached,
  fetchVerifiedNodeShasumsFileCached,
  RUNTIME_SHASUMS_DIR,
} from '@pnpm/crypto.shasums-file'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import * as openpgp from 'openpgp'

function temporaryDirectory (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-shasums-cache-'))
}

const SHASUMS_URL = 'https://nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt'
const SHASUMS = 'deadbeef'.repeat(8) + '  node-v22.11.0-darwin-arm64.tar.gz\n'

function countingFetch (responses: Record<string, () => Response>): { fetch: FetchFromRegistry, calls: string[] } {
  const calls: string[] = []
  const fetch = (async (url: string) => {
    calls.push(url)
    const response = responses[url]
    if (!response) return new Response(null, { status: 404 })
    return response()
  }) as unknown as FetchFromRegistry
  return { fetch, calls }
}

test('fetchShasumsFileCached() serves repeat reads from the cache', async () => {
  const cacheDir = temporaryDirectory()
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(SHASUMS),
  })

  const fetched = await fetchShasumsFileCached(fetch, SHASUMS_URL, cacheDir)
  const cached = await fetchShasumsFileCached(fetch, SHASUMS_URL, cacheDir)

  expect(cached).toStrictEqual(fetched)
  expect(calls).toStrictEqual([SHASUMS_URL])
  expect(fs.readFileSync(
    path.join(cacheDir, RUNTIME_SHASUMS_DIR, 'nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt'),
    'utf8'
  )).toBe(SHASUMS)
})

test('fetchVerifiedNodeShasumsFileCached() caches the body after verification and skips re-verification', async () => {
  const { privateKey, publicKey } = await openpgp.generateKey({
    userIDs: [{ name: 'Test Node Releaser', email: 'test@nodejs.example' }],
    format: 'armored',
  })
  const signingKey = await openpgp.readPrivateKey({ armoredKey: privateKey })
  const message = await openpgp.createMessage({ binary: new TextEncoder().encode(SHASUMS) })
  const signature = await openpgp.sign({ message, signingKeys: signingKey, detached: true, format: 'binary' }) as Uint8Array
  const trustedKeys = [{ armoredKey: publicKey }]
  const cacheDir = temporaryDirectory()
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
    [`${SHASUMS_URL}.sig`]: () => new Response(signature.slice().buffer as ArrayBuffer),
  })

  const fetched = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, cacheDir, trustedKeys)
  const cached = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, cacheDir, trustedKeys)

  expect(cached).toStrictEqual(fetched)
  expect(calls).toStrictEqual([SHASUMS_URL, `${SHASUMS_URL}.sig`])
})

test('fetchVerifiedNodeShasumsFileCached() does not cache a body that failed verification', async () => {
  const { publicKey } = await openpgp.generateKey({
    userIDs: [{ name: 'Test Node Releaser', email: 'test@nodejs.example' }],
    format: 'armored',
  })
  const trustedKeys = [{ armoredKey: publicKey }]
  const cacheDir = temporaryDirectory()
  const { fetch } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
  })

  for (let attempt = 0; attempt < 2; attempt++) {
    // eslint-disable-next-line no-await-in-loop
    await expect(fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, cacheDir, trustedKeys)).rejects.toThrow()
  }
  expect(fs.existsSync(path.join(cacheDir, RUNTIME_SHASUMS_DIR))).toBe(false)
})

test('a URL the cache path mapping cannot represent is fetched every time', async () => {
  const cacheDir = temporaryDirectory()
  const url = 'https://nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt?token=1'
  const { fetch, calls } = countingFetch({
    [url]: () => new Response(SHASUMS),
  })

  await fetchShasumsFileCached(fetch, url, cacheDir)
  await fetchShasumsFileCached(fetch, url, cacheDir)

  expect(calls).toStrictEqual([url, url])
})
