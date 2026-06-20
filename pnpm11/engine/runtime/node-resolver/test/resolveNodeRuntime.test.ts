import { expect, test } from '@jest/globals'
import type { FetchFromRegistry } from '@pnpm/fetching.types'

import { resolveNodeRuntime } from '../lib/index.js'

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
