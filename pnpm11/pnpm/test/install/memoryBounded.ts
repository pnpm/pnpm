import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

const PACKAGE_COUNT = 40
const JUNK_BYTES = 6 * 1024 * 1024

/**
 * End-to-end regression guard for https://github.com/pnpm/pnpm/issues/8441:
 * a whole `pnpm install` (the bundled CLI, not just the resolver package)
 * must not retain full-document bulk, so it completes inside a small heap.
 *
 * The registry documents — served by a local server, one per optional
 * dependency so the CLI fetches full metadata — each carry 6 MB of
 * install-irrelevant bulk, 240 MB total. Retaining the parsed documents,
 * as the resolver did before condensing, overflows the 256 MB cap by
 * ~100 MB regardless of which cache pins them; the condensed working set
 * plus the CLI's baseline fits with roughly 100 MB to spare (the fixed
 * install passes even a 192 MB cap). That margin in both directions keeps the
 * guard deterministic rather than timing-sensitive.
 */
test('install --lockfile-only completes within a small heap while registry documents carry megabytes of bulk', async () => {
  const server = await startBloatedRegistry()
  const optionalDependencies = Object.fromEntries(
    Array.from({ length: PACKAGE_COUNT }, (_, i) => [`bloated-pkg-${i}`, '1.0.0'])
  )
  const project = prepare({ optionalDependencies })

  try {
    await execPnpm(['install', '--lockfile-only'], {
      env: {
        NODE_OPTIONS: '--max-old-space-size=256',
        pnpm_config_registry: server.url,
        // Bounds the post-fix transient footprint (response body + freshly
        // parsed document per in-flight request) so the heap cap measures
        // what is RETAINED across the resolution, which no concurrency
        // setting can shrink.
        pnpm_config_network_concurrency: '4',
      },
    })
  } finally {
    server.close()
  }

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toHaveLength(PACKAGE_COUNT)
  expect(lockfile.packages['bloated-pkg-0@1.0.0'].libc).toEqual(['glibc'])
})

interface BloatedRegistry {
  url: string
  close: () => void
}

// Serves a full packument for any package name requested. The junk string is
// shared server-side; JSON.parse in the CLI gives each document its own copy —
// the copy whose retention this test bounds. Tarball URLs point back at this
// server but are never fetched: --lockfile-only resolves without downloading.
async function startBloatedRegistry (): Promise<BloatedRegistry> {
  const junk = 'x'.repeat(JUNK_BYTES / 2)
  const server = http.createServer((req, res) => {
    const name = decodeURIComponent((req.url ?? '').slice(1))
    res.setHeader('content-type', 'application/json')
    res.end(JSON.stringify({
      name,
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name,
          version: '1.0.0',
          libc: ['glibc'],
          description: junk,
          dist: {
            tarball: `${registryUrl()}${name}/-/${name}-1.0.0.tgz`,
            integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
          },
        },
      },
      readme: junk,
    }))
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  function registryUrl (): string {
    return `http://127.0.0.1:${(server.address() as AddressInfo).port}/`
  }
  return {
    url: registryUrl(),
    close: () => {
      server.close()
    },
  }
}
