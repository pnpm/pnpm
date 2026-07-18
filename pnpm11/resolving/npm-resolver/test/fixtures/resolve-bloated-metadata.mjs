// Child of memoryBounded.test.ts: resolves PACKAGE_COUNT optional
// dependencies — each served as a full document carrying JUNK_BYTES of
// install-irrelevant bulk — under the small heap the parent capped this
// process to (see the test for the sizing arithmetic). Imports the compiled
// lib because it runs outside jest's TS transform.
import { mkdtempSync, rmSync } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { createFetchFromRegistry } from '@pnpm/network.fetch'

import { createNpmResolver } from '../../lib/index.js'

const PACKAGE_COUNT = 30
const JUNK_BYTES = 4 * 1024 * 1024

// One shared junk string keeps the server side of this process cheap; each
// response embeds it, and JSON.parse gives the resolver its own copy per
// document — the copy whose retention this test exists to bound.
const junk = 'x'.repeat(JUNK_BYTES / 2)

function packumentFor (name) {
  return JSON.stringify({
    name,
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        libc: ['glibc'],
        description: junk,
        dist: {
          tarball: `https://registry.example.test/${name}/-/${name}-1.0.0.tgz`,
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
    readme: junk,
  })
}

const server = http.createServer((req, res) => {
  const name = decodeURIComponent(req.url.slice(1))
  res.setHeader('content-type', 'application/json')
  res.end(packumentFor(name))
})
await new Promise((resolve) => {
  server.listen(0, '127.0.0.1', resolve)
})
const registry = `http://127.0.0.1:${server.address().port}/`

const cacheDir = mkdtempSync(path.join(os.tmpdir(), 'pnpm-memory-bounded-'))
const { resolveFromNpm } = createNpmResolver(createFetchFromRegistry({}), () => undefined, {
  cacheDir,
  registries: { default: registry },
})

// Sequential on purpose: transient memory (response body + freshly parsed
// document) then stays around one document, so the heap cap measures what is
// RETAINED across resolutions, not the fetch concurrency.
for (let i = 0; i < PACKAGE_COUNT; i++) {
  const result = await resolveFromNpm(
    { alias: `bloated-pkg-${i}`, bareSpecifier: '1.0.0', optional: true },
    {}
  )
  if (result?.manifest?.version !== '1.0.0' || result.manifest.libc?.[0] !== 'glibc') {
    throw new Error(`bloated-pkg-${i} resolved incorrectly: ${JSON.stringify(result?.manifest)}`)
  }
}

server.close()
rmSync(cacheDir, { recursive: true, force: true })
