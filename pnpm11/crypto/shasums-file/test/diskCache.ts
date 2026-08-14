import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import {
  fetchShasumsFileCached,
  fetchVerifiedNodeShasumsFileCached,
  RUNTIME_SHASUMS_DIR,
} from '@pnpm/crypto.shasums-file'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import * as openpgp from 'openpgp'

const SHASUMS_URL = 'https://nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt'
const SHASUMS = 'deadbeef'.repeat(8) + '  node-v22.11.0-darwin-arm64.tar.gz\n'

afterAll(async () => {
  await Promise.all(temporaryDirectories.map((dir) => fs.promises.rm(dir, { recursive: true, force: true })))
})

test('fetchShasumsFileCached() serves repeat reads from the cache', async () => {
  const cacheDir = await temporaryDirectory()
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(SHASUMS),
  })

  const fetched = await fetchShasumsFileCached(fetch, SHASUMS_URL, { cacheDir })
  const cached = await fetchShasumsFileCached(fetch, SHASUMS_URL, { cacheDir })

  expect(cached).toStrictEqual(fetched)
  expect(calls).toStrictEqual([SHASUMS_URL])
  expect(await fs.promises.readFile(
    path.join(cacheDir, RUNTIME_SHASUMS_DIR, 'unverified/nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt'),
    'utf8'
  )).toBe(SHASUMS)
})

test('fetchVerifiedNodeShasumsFileCached() caches the body after verification and skips re-verification', async () => {
  const { signature, trustedKeys } = await signedShasums()
  const cacheDir = await temporaryDirectory()
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
    [`${SHASUMS_URL}.sig`]: () => new Response(signature.slice().buffer as ArrayBuffer),
  })

  const fetched = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })
  const cached = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })

  expect(cached).toStrictEqual(fetched)
  expect(calls).toStrictEqual([SHASUMS_URL, `${SHASUMS_URL}.sig`])
})

test('fetchVerifiedNodeShasumsFileCached() does not cache a body that failed verification', async () => {
  const { trustedKeys } = await signedShasums()
  const cacheDir = await temporaryDirectory()
  const { fetch } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
  })

  for (let attempt = 0; attempt < 2; attempt++) {
    // eslint-disable-next-line no-await-in-loop
    await expect(fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })).rejects.toThrow()
  }
  expect(fs.existsSync(path.join(cacheDir, RUNTIME_SHASUMS_DIR))).toBe(false)
})

// The two trust classes cache into disjoint subtrees: a body written by an
// unverified fetch must never satisfy a reader that expects a
// signature-verified body.
test('an unverified cache entry does not serve a verified read', async () => {
  const { signature, trustedKeys } = await signedShasums()
  const cacheDir = await temporaryDirectory()
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
    [`${SHASUMS_URL}.sig`]: () => new Response(signature.slice().buffer as ArrayBuffer),
  })

  await fetchShasumsFileCached(fetch, SHASUMS_URL, { cacheDir })
  await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })

  // The verified read went to the network (and verified) despite the
  // unverified entry for the same URL.
  expect(calls).toStrictEqual([SHASUMS_URL, SHASUMS_URL, `${SHASUMS_URL}.sig`])
})

// The cache directory is project-configurable, so a verified hit must prove
// itself against the trusted keys on every read: a pre-seeded entry without a
// valid persisted signature is a miss and gets refetched.
test('a seeded verified cache entry without a valid signature is refetched', async () => {
  const { signature, trustedKeys } = await signedShasums()
  const cacheDir = await temporaryDirectory()
  const entryDir = path.join(cacheDir, RUNTIME_SHASUMS_DIR, 'verified/nodejs.example.test/download/release/v22.11.0')
  await fs.promises.mkdir(entryDir, { recursive: true })
  await fs.promises.writeFile(path.join(entryDir, 'SHASUMS256.txt'), '0'.repeat(64) + '  node-v22.11.0-darwin-arm64.tar.gz\n')
  await fs.promises.writeFile(path.join(entryDir, 'SHASUMS256.txt.sig'), 'not a signature')
  const { fetch, calls } = countingFetch({
    [SHASUMS_URL]: () => new Response(new TextEncoder().encode(SHASUMS)),
    [`${SHASUMS_URL}.sig`]: () => new Response(signature.slice().buffer as ArrayBuffer),
  })

  const refetched = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })
  const cached = await fetchVerifiedNodeShasumsFileCached(fetch, SHASUMS_URL, { cacheDir, trustedKeys })

  // The seeded pair was rejected and replaced by the genuine fetched pair,
  // which then serves the second read from the cache.
  expect(refetched[0].integrity).not.toContain('AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=')
  expect(cached).toStrictEqual(refetched)
  expect(calls).toStrictEqual([SHASUMS_URL, `${SHASUMS_URL}.sig`])
})

test('a URL the cache path mapping cannot represent is fetched every time', async () => {
  const cacheDir = await temporaryDirectory()
  const url = 'https://nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt?token=1'
  const { fetch, calls } = countingFetch({
    [url]: () => new Response(SHASUMS),
  })

  await fetchShasumsFileCached(fetch, url, { cacheDir })
  await fetchShasumsFileCached(fetch, url, { cacheDir })

  expect(calls).toStrictEqual([url, url])
})

const temporaryDirectories: string[] = []

async function temporaryDirectory (): Promise<string> {
  const dir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-shasums-cache-'))
  temporaryDirectories.push(dir)
  return dir
}

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

async function signedShasums (): Promise<{ signature: Uint8Array, trustedKeys: Array<{ armoredKey: string }> }> {
  const { privateKey, publicKey } = await openpgp.generateKey({
    userIDs: [{ name: 'Test Node Releaser', email: 'test@nodejs.example' }],
    format: 'armored',
  })
  const signingKey = await openpgp.readPrivateKey({ armoredKey: privateKey })
  const message = await openpgp.createMessage({ binary: new TextEncoder().encode(SHASUMS) })
  const signature = await openpgp.sign({ message, signingKeys: signingKey, detached: true, format: 'binary' }) as Uint8Array
  return { signature, trustedKeys: [{ armoredKey: publicKey }] }
}
