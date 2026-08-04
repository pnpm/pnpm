import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

const PACKAGE_COUNT = 40
const JUNK_BYTES = 6 * 1024 * 1024

/**
 * End-to-end regression guard for https://github.com/pnpm/pnpm/issues/8441:
 * the bundled CLI installs 40 optional dependencies (so it fetches full
 * metadata) whose documents carry 240 MB of install-irrelevant bulk under a
 * 256 MB heap cap. Retaining the parsed documents overshoots the cap by
 * ~100 MB regardless of which cache pins them, while the condensed install
 * passes even a 192 MB cap, so the guard is deterministic rather than
 * timing-sensitive.
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
        // So the heap cap measures what is retained across the resolution
        // rather than in-flight transients.
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
// shared server-side; JSON.parse in the CLI gives each document its own copy.
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
