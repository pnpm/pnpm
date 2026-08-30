import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { resolveAndInstallConfigDeps } from '@pnpm/installing.env-installer'
import { createEnvLockfile, readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { type LogBase, streamParser } from '@pnpm/logger'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { loadJsonFileSync } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

function createOpts (registryUrl: string = registry) {
  const { storeController, storeDir } = createTempStore()
  return {
    registriesByScope: { default: registryUrl },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    userConfig: {},
    store: storeController,
    storeDir,
  }
}

interface InstallingConfigDepsEvent { status: string, deps?: Array<{ name: string, version: string }> }

// `streamParser` is a `split2` Transform stream that buffers writes until the
// first 'data' listener attaches, then drains the whole buffer into it.
// Subscribing per-test would therefore replay events from earlier tests into
// the current test's listener. Subscribe once at module load and let each test
// take only the events accumulated since its last drain.
const accumulatedConfigDepEvents: InstallingConfigDepsEvent[] = []
const configDepsListener = (msg: LogBase): void => {
  const log = msg as { name?: string, status?: string, deps?: Array<{ name: string, version: string }> }
  if (log.name !== 'pnpm:installing-config-deps' || log.status == null) return
  accumulatedConfigDepEvents.push({ status: log.status, deps: log.deps })
}
streamParser.on('data', configDepsListener)
afterAll(() => {
  streamParser.removeListener('data', configDepsListener)
})

function takeConfigDepEvents (): InstallingConfigDepsEvent[] {
  return accumulatedConfigDepEvents.splice(0, accumulatedConfigDepEvents.length)
}

test('resolves and installs config dep when no env lockfile exists', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Simulate a config dep manually added to pnpm-workspace.yaml (clean specifier, no lockfile)
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  // Package should be installed
  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')

  // Env lockfile should be created with resolved info
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })
})

test('resolves newly added config dep when env lockfile already has other deps', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create env lockfile with one dep
  const existingLockfile = createEnvLockfile()
  existingLockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  existingLockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  existingLockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), existingLockfile)

  // Now install with an additional dep
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
    '@pnpm.e2e/bar': '100.0.0',
  }, opts)

  // Both packages should be installed
  const fooManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(fooManifest.name).toBe('@pnpm.e2e/foo')
  expect(fooManifest.version).toBe('100.0.0')

  const barManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/bar/package.json')
  expect(barManifest.name).toBe('@pnpm.e2e/bar')
  expect(barManifest.version).toBe('100.0.0')

  // Env lockfile should have both deps
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toBeDefined()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/bar']).toBeDefined()
})

test('skips resolution when all deps are already in env lockfile', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create complete env lockfile
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  // Install should work without network (using lockfile data)
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})

test('re-resolves and reinstalls when config dep version changes in pnpm-workspace.yaml', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create env lockfile with foo@100.0.0
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  // Install first with the old version
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  // Now simulate user changing the version in pnpm-workspace.yaml
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.1.0',
  }, opts)

  // The new version should be installed
  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.1.0')

  // Env lockfile should be updated with the new version
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.1.0',
    version: '100.1.0',
  })
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.1.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.1.0'),
    },
  })
  // Old version should be cleaned up from the lockfile
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toBeUndefined()
})

test('handles old format config deps via migration path', async () => {
  prepareEmpty()
  const opts = createOpts()

  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }, opts)

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})

test('takes the tarball of an old-format config dep from the packument', async () => {
  prepareEmpty()
  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')

  let advertisedTarball = ''
  await withNonDerivableTarballRegistry(async (registryUrl) => {
    advertisedTarball = `${registryUrl}tarballs/@pnpm.e2e/foo/-/foo-100.0.0.tgz`
    await resolveAndInstallConfigDeps({
      '@pnpm.e2e/foo': `100.0.0+${integrity}`,
    }, createOpts(registryUrl))
  })

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0'].resolution).toStrictEqual({
    integrity,
    tarball: advertisedTarball,
  })
})

// Stands up a registry that serves its tarballs from a path pnpm cannot derive
// from the package name, version, and registry URL — the shape of GitLab's npm
// registry. Packuments are proxied from pnpr with their `dist.tarball` pointed
// at a `/tarballs/` prefix, and the derivable URL is answered with a 404.
async function withNonDerivableTarballRegistry (run: (registryUrl: string) => Promise<void>): Promise<void> {
  const upstreamBase = `http://localhost:${REGISTRY_MOCK_PORT}`
  let proxyBase = ''
  const server = http.createServer((req, res) => {
    void (async () => {
      const tarballPath = req.url!.startsWith('/tarballs/') ? req.url!.slice('/tarballs'.length) : undefined
      if (tarballPath == null && req.url!.endsWith('.tgz')) {
        res.writeHead(404)
        res.end('Not Found')
        return
      }
      const upstream = await fetch(`${upstreamBase}${tarballPath ?? req.url!}`, {
        headers: { accept: req.headers.accept ?? '*/*' },
      })
      const contentType = upstream.headers.get('content-type') ?? ''
      if (contentType.includes('json')) {
        const body = (await upstream.text()).split(upstreamBase).join(`${proxyBase}/tarballs`)
        res.writeHead(upstream.status, { 'content-type': 'application/json' })
        res.end(body)
      } else {
        res.writeHead(upstream.status, { 'content-type': contentType })
        res.end(Buffer.from(await upstream.arrayBuffer()))
      }
    })().catch((err: unknown) => {
      res.writeHead(500)
      res.end(String(err))
    })
  })
  await new Promise<void>((resolve) => {
    server.listen(0, resolve)
  })
  proxyBase = `http://localhost:${(server.address() as AddressInfo).port}`
  try {
    await run(`${proxyBase}/`)
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err == null) {
          resolve()
        } else {
          reject(err)
        }
      })
    })
  }
}

test('keeps optional subdeps of a pinned config dep out of the lockfile', async () => {
  prepareEmpty()
  const opts = createOpts()

  // @pnpm.e2e/foobar declares `@pnpm.e2e/bar: "^100.0.0"` as an
  // optionalDependency — a range, which a clean specifier forbids.
  const integrity = getIntegrity('@pnpm.e2e/foobar', '100.0.0')
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foobar': `100.0.0+${integrity}`,
  }, opts)

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foobar/package.json')
  expect(manifest.version).toBe('100.0.0')

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.snapshots['@pnpm.e2e/foobar@100.0.0']).toStrictEqual({})
})

test('handles mixed old-format and new-format config deps together', async () => {
  prepareEmpty()
  const opts = createOpts()

  // One dep in old inline-integrity format, another as a clean specifier
  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
    '@pnpm.e2e/bar': '100.0.0',
  }, opts)

  // Both packages should be installed
  const fooManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(fooManifest.name).toBe('@pnpm.e2e/foo')
  expect(fooManifest.version).toBe('100.0.0')

  const barManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/bar/package.json')
  expect(barManifest.name).toBe('@pnpm.e2e/bar')
  expect(barManifest.version).toBe('100.0.0')

  // Env lockfile should have both deps
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toBeDefined()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/bar']).toBeDefined()
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toBeDefined()
  expect(envLockfile!.packages['@pnpm.e2e/bar@100.0.0']).toBeDefined()
})

test('fails with frozenLockfile when old-format deps need migration', async () => {
  prepareEmpty()
  const opts = createOpts()

  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await expect(resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }, { ...opts, frozenLockfile: true })).rejects.toThrow('Cannot update configDependencies with "frozen-lockfile"')
})

test('fails with frozenLockfile when new-format deps need resolution', async () => {
  prepareEmpty()
  const opts = createOpts()

  await expect(resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, { ...opts, frozenLockfile: true })).rejects.toThrow('Cannot update configDependencies with "frozen-lockfile"')
})

test('emits installing-config-deps events only when work is needed', async () => {
  prepareEmpty()
  const opts = createOpts()

  takeConfigDepEvents()
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)
  const firstRunEvents = takeConfigDepEvents()

  expect(firstRunEvents.map(e => e.status)).toEqual(['started', 'done'])
  expect(firstRunEvents.find(e => e.status === 'done')?.deps).toEqual([
    { name: '@pnpm.e2e/foo', version: '100.0.0' },
  ])

  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)
  const secondRunEvents = takeConfigDepEvents()

  expect(secondRunEvents).toStrictEqual([])
})

test('succeeds with frozenLockfile when env lockfile is up-to-date', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create complete env lockfile
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, { ...opts, frozenLockfile: true })

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})
