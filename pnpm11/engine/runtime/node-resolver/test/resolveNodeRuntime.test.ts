import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { PlatformAssetResolution } from '@pnpm/resolving.resolver-base'

import { DEFAULT_NODE_MIRROR_BASE_URL, resolveNodeRuntime, resolveNodeVersion, UNOFFICIAL_NODE_MIRROR_BASE_URL } from '../lib/index.js'

const MIRROR = 'https://node.example/download/rc/'

const fetch: FetchFromRegistry = async (url) => {
  switch (url) {
    case `${MIRROR}index.json`:
      return new Response(JSON.stringify([
        { version: 'v22.11.0', lts: false },
        { version: 'v22.10.0', lts: false },
      ]))
    case `${MIRROR}v22.11.0/SHASUMS256.txt`:
      return new Response('ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz\n')
    default:
      throw new Error(`Unexpected URL: ${url}`)
  }
}

test.each([
  ['runtime:rc/22', undefined, 'runtime:22.11.0'],
  ['runtime:rc/^22', undefined, 'runtime:^22.11.0'],
  ['runtime:rc/22', 'runtime:~22.0.0', 'runtime:~22.11.0'],
  ['runtime:rc/^22', 'runtime:22.0.0', 'runtime:22.11.0'],
])('resolveNodeRuntime() preserves runtime version prefix (%s, previous %s)', async (bareSpecifier, prevSpecifier, expected) => {
  const resolution = await resolveNodeRuntime({
    fetchFromRegistry: fetch,
    nodeDownloadMirrors: {
      rc: MIRROR,
    },
  }, {
    alias: 'node',
    bareSpecifier,
    prevSpecifier,
  })

  expect(resolution?.normalizedBareSpecifier).toBe(expected)
})

test('resolveNodeRuntime() authenticates release index and SHASUMS requests without sharing cached metadata', async () => {
  const requests: Array<{ url: string, authHeaderValue?: string }> = []
  const authenticatedFetch: FetchFromRegistry = async (url, opts) => {
    requests.push({ url, authHeaderValue: opts?.authHeaderValue })
    return fetch(url)
  }
  const cacheDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-node-resolver-auth-'))
  try {
    for (let run = 0; run < 2; run++) {
      // eslint-disable-next-line no-await-in-loop
      await resolveNodeRuntime({
        fetchFromRegistry: authenticatedFetch,
        getAuthHeader: url => url.startsWith(MIRROR) ? 'Bearer mirror-token' : undefined,
        nodeDownloadMirrors: { rc: MIRROR },
        cacheDir,
      }, {
        alias: 'node',
        bareSpecifier: 'runtime:rc/22',
      })
    }

    expect(requests).toEqual([
      { url: `${MIRROR}index.json`, authHeaderValue: 'Bearer mirror-token' },
      { url: `${MIRROR}v22.11.0/SHASUMS256.txt`, authHeaderValue: 'Bearer mirror-token' },
      { url: `${MIRROR}index.json`, authHeaderValue: 'Bearer mirror-token' },
      { url: `${MIRROR}v22.11.0/SHASUMS256.txt`, authHeaderValue: 'Bearer mirror-token' },
    ])
  } finally {
    await fs.promises.rm(cacheDir, { recursive: true, force: true })
  }
})

test.each([
  'http://node.example/download/rc/',
  'http://127.attacker.example/download/rc/',
])('resolveNodeRuntime() omits credentials for remote HTTP mirror %s', async (mirror) => {
  const requests: Array<{ url: string, authHeaderValue?: string }> = []
  const httpFetch: FetchFromRegistry = async (url, opts) => {
    requests.push({ url, authHeaderValue: opts?.authHeaderValue })
    if (url === `${mirror}index.json`) {
      return new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }]))
    }
    if (url === `${mirror}v22.11.0/SHASUMS256.txt`) {
      return new Response('ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz\n')
    }
    throw new Error(`Unexpected URL: ${url}`)
  }

  await resolveNodeRuntime({
    fetchFromRegistry: httpFetch,
    getAuthHeader: () => 'Bearer mirror-token',
    nodeDownloadMirrors: { rc: mirror },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:rc/22',
  })

  expect(requests).toEqual([
    { url: `${mirror}index.json`, authHeaderValue: undefined },
    { url: `${mirror}v22.11.0/SHASUMS256.txt`, authHeaderValue: undefined },
  ])
})

test('resolveNodeVersion() selects credentials again after a redirect', async () => {
  const startUrl = `${MIRROR}index.json`
  const redirectedUrl = 'http://node.example/public/index.json'
  const requests: Array<{ url: string, authHeaderValue?: string }> = []
  const redirectingFetch: FetchFromRegistry = async (url, opts) => {
    requests.push({ url, authHeaderValue: opts?.authHeaderValue })
    if (url === startUrl) {
      return new Response(null, { status: 302, headers: { location: redirectedUrl } })
    }
    if (url === redirectedUrl) {
      return new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }]))
    }
    throw new Error(`Unexpected URL: ${url}`)
  }

  const version = await resolveNodeVersion(redirectingFetch, 'latest', {
    nodeMirrorBaseUrl: MIRROR,
    getAuthHeader: url => url.startsWith(MIRROR) ? 'Bearer mirror-token' : undefined,
  })

  expect(version).toBe('22.11.0')
  expect(requests).toEqual([
    { url: startUrl, authHeaderValue: 'Bearer mirror-token' },
    { url: redirectedUrl, authHeaderValue: undefined },
  ])
})

const RELEASE_MIRROR = 'https://node.example/download/release/'

// An exact-specifier resolve skips the release index, so a nonexistent
// version first fails its asset fetch; the resolver must then consult the
// index and raise the canonical not-found error rather than the raw fetch
// failure.
test('resolveNodeRuntime() raises NODEJS_VERSION_NOT_FOUND for a nonexistent exact version', async () => {
  const { fetch: countedFetch, calls } = countingFetch({
    [`${RELEASE_MIRROR}index.json`]: () => new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }])),
  })

  await expect(resolveNodeRuntime({
    fetchFromRegistry: countedFetch,
    nodeDownloadMirrors: { release: RELEASE_MIRROR },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:22.99.0',
  })).rejects.toThrow(/Could not find a Node.js version that satisfies 22.99.0/)
  // The asset fetch runs first; the index is only consulted to classify the
  // failure.
  expect(calls[0]).toBe(`${RELEASE_MIRROR}v22.99.0/SHASUMS256.txt`)
  expect(calls).toContain(`${RELEASE_MIRROR}index.json`)
})

// When the index confirms the exact version exists, the asset-fetch failure
// is the real error and must surface unchanged.
test('resolveNodeRuntime() keeps the asset error when the exact version exists', async () => {
  const { fetch: countedFetch } = countingFetch({
    [`${RELEASE_MIRROR}index.json`]: () => new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }])),
    [`${RELEASE_MIRROR}v22.11.0/SHASUMS256.txt`]: () => new Response(null, { status: 500 }),
  })

  await expect(resolveNodeRuntime({
    fetchFromRegistry: countedFetch,
    nodeDownloadMirrors: { release: RELEASE_MIRROR },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:22.11.0',
  })).rejects.toThrow(/SHASUMS256.txt/)
})

// A SHASUMS body cached by an earlier resolve serves the next one without
// refetching it; only the (mutable) release index is fetched again.
test('resolveNodeRuntime() serves repeat asset reads from the cache', async () => {
  const cacheDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-node-resolver-'))
  try {
    const { fetch: countedFetch, calls } = countingFetch({
      [`${MIRROR}index.json`]: () => new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }])),
      [`${MIRROR}v22.11.0/SHASUMS256.txt`]: () => new Response('ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz\n'),
    })

    for (let run = 0; run < 2; run++) {
      // eslint-disable-next-line no-await-in-loop
      const resolution = await resolveNodeRuntime({
        fetchFromRegistry: countedFetch,
        nodeDownloadMirrors: { rc: MIRROR },
        cacheDir,
      }, {
        alias: 'node',
        bareSpecifier: 'runtime:rc/22',
      })
      expect(resolution?.resolution.variants).toHaveLength(1)
    }

    expect(calls.filter((url) => url === `${MIRROR}v22.11.0/SHASUMS256.txt`)).toHaveLength(1)
    expect(calls.filter((url) => url === `${MIRROR}index.json`)).toHaveLength(2)
  } finally {
    await fs.promises.rm(cacheDir, { recursive: true, force: true })
  }
})

function countingFetch (responses: Record<string, () => Response>): { fetch: FetchFromRegistry, calls: string[] } {
  const calls: string[] = []
  const countedFetch = (async (url: string) => {
    calls.push(url)
    const response = responses[url]
    if (!response) return new Response(null, { status: 404 })
    return response()
  }) as unknown as FetchFromRegistry
  return { fetch: countedFetch, calls }
}

const OFFICIAL_INDEX_URL = `${DEFAULT_NODE_MIRROR_BASE_URL}index.json`
const OFFICIAL_SHASUMS_URL = `${DEFAULT_NODE_MIRROR_BASE_URL}v22.11.0/SHASUMS256.txt`
const UNOFFICIAL_SHASUMS_URL = `${UNOFFICIAL_NODE_MIRROR_BASE_URL}v22.11.0/SHASUMS256.txt`

test('resolveNodeRuntime() skips the musl assets of a release unofficial-builds never built', async () => {
  const variants = await resolveMuslVariants(async () => new Response(null, { status: 404 }))

  expect(variants.map(({ targets }) => targets[0].libc)).toStrictEqual([undefined])
})

test('resolveNodeRuntime() reads the musl assets unofficial-builds publishes', async () => {
  const variants = await resolveMuslVariants(async () => new Response(MUSL_SHASUMS))

  expect(variants.map(({ targets }) => targets[0].libc)).toStrictEqual([undefined, 'musl'])
})

test.each([
  403,
  500,
])('resolveNodeRuntime() fails when unofficial-builds answers %i', async (status) => {
  await expect(resolveMuslVariants(async () => new Response(null, { status })))
    .rejects.toThrow(`Failed to fetch integrity file: ${UNOFFICIAL_SHASUMS_URL} (status: ${status})`)
})

test('resolveNodeRuntime() fails when unofficial-builds cannot be reached', async () => {
  await expect(resolveMuslVariants(() => Promise.reject(new Error('getaddrinfo ENOTFOUND unofficial-builds.nodejs.org'))))
    .rejects.toThrow('getaddrinfo ENOTFOUND unofficial-builds.nodejs.org')
})

/**
 * The musl request is only made when the picked mirror is the default one, and
 * pointing the `rc` channel at it keeps that mirror's own SHASUMS file
 * unsigned, so no release signature has to be minted for the official half.
 */
async function resolveMuslVariants (respondToMuslRequest: () => Promise<Response>): Promise<PlatformAssetResolution[]> {
  const fetch: FetchFromRegistry = async (url) => {
    switch (url) {
      case OFFICIAL_INDEX_URL:
        return new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }]))
      case OFFICIAL_SHASUMS_URL:
        return new Response(GLIBC_SHASUMS)
      case UNOFFICIAL_SHASUMS_URL:
        return respondToMuslRequest()
      default:
        throw new Error(`Unexpected URL: ${url}`)
    }
  }
  const resolution = await resolveNodeRuntime({
    fetchFromRegistry: fetch,
    nodeDownloadMirrors: { rc: DEFAULT_NODE_MIRROR_BASE_URL },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:rc/22.11.0',
  })
  return resolution!.resolution.variants
}

const GLIBC_SHASUMS = 'ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz\n'
const MUSL_SHASUMS = '696cb00a4b9d0e4dd2eb95e5fe32e8ff1ac2c3dfe54c7a2a5f03f7f9e6f0b1c2  node-v22.11.0-linux-x64-musl.tar.gz\n'

test('resolveNodeRuntime() resolves native win-arm64 variant for Node 20+', async () => {
  const fetch: FetchFromRegistry = async (url) => {
    if (url === `${MIRROR}index.json`) {
      return new Response(JSON.stringify([{ version: 'v22.11.0', lts: false }]))
    }
    if (url === `${MIRROR}v22.11.0/SHASUMS256.txt`) {
      return new Response(
        'b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2  node-v22.11.0-win-x64.zip\n' +
        'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1  node-v22.11.0-win-arm64.zip\n'
      )
    }
    throw new Error(`Unexpected URL: ${url}`)
  }

  const resolution = await resolveNodeRuntime({
    fetchFromRegistry: fetch,
    nodeDownloadMirrors: { rc: MIRROR },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:rc/22.11.0',
  })

  const variants = resolution!.resolution.variants
  const winArm64 = variants.find(v => v.targets.some(t => t.os === 'win32' && t.cpu === 'arm64'))
  expect(winArm64).toBeDefined()
  expect(winArm64!.targets).toStrictEqual([{ os: 'win32', cpu: 'arm64' }])
  expect(winArm64!.resolution).toMatchObject({
    url: `${MIRROR}v22.11.0/node-v22.11.0-win-arm64.zip`,
    archive: 'zip',
    prefix: 'node-v22.11.0-win-arm64',
  })
})

test('resolveNodeRuntime() includes win32-arm64 target on win-x64 variant for Node < 20', async () => {
  const fetch: FetchFromRegistry = async (url) => {
    if (url === `${MIRROR}index.json`) {
      return new Response(JSON.stringify([{ version: 'v18.20.0', lts: false }]))
    }
    if (url === `${MIRROR}v18.20.0/SHASUMS256.txt`) {
      return new Response('c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3  node-v18.20.0-win-x64.zip\n')
    }
    throw new Error(`Unexpected URL: ${url}`)
  }

  const resolution = await resolveNodeRuntime({
    fetchFromRegistry: fetch,
    nodeDownloadMirrors: { rc: MIRROR },
  }, {
    alias: 'node',
    bareSpecifier: 'runtime:rc/18.20.0',
  })

  const variants = resolution!.resolution.variants
  expect(variants).toHaveLength(1)
  expect(variants[0].targets).toStrictEqual([
    { os: 'win32', cpu: 'x64' },
    { os: 'win32', cpu: 'arm64' },
  ])
  expect(variants[0].resolution).toMatchObject({
    url: `${MIRROR}v18.20.0/node-v18.20.0-win-x64.zip`,
    archive: 'zip',
    prefix: 'node-v18.20.0-win-x64',
  })
})

