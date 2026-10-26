// A plain lockfile key names no registry. When its scope is routed to another
// registry, a resolving install resolves the package and its locked children
// again from the new registry, so a version the new registry does not serve
// is not pinned from the old one.
import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

/**
 * `@pnpm.e2e/foobar@100.0.0` declares `@pnpm.e2e/foo: ^100.0.0`, a plain range
 * the `@pnpm.e2e` scope routes.
 */
const PARENT = '@pnpm.e2e/foobar'
const CHILD = '@pnpm.e2e/foo'
/** The version of `CHILD` the other registry never published. */
const UNPUBLISHED = '100.1.0'

interface OtherRegistry {
  url: string
  close: () => Promise<void>
}

let other: OtherRegistry

beforeAll(async () => {
  other = await startRegistryWithoutVersion(CHILD, UNPUBLISHED)
})

afterAll(async () => {
  await other.close()
})

interface Packument {
  versions: Record<string, unknown>
  'dist-tags'?: Record<string, string>
  time?: Record<string, string>
}

/**
 * Serves the mock registry's npm endpoints under another origin, which makes
 * it another registry, with one version of one package left out of its
 * metadata. Tarball URLs in the packuments name the mock registry itself.
 */
async function startRegistryWithoutVersion (pkgName: string, version: string): Promise<OtherRegistry> {
  const packumentPath = `/${pkgName}`
  const server = http.createServer((req, res) => {
    const isPackument = decodeURIComponent(req.url ?? '') === packumentPath
    const upstream = http.request({
      host: 'localhost',
      port: REGISTRY_MOCK_PORT,
      method: req.method,
      path: `/npm${req.url}`,
      headers: { ...req.headers, ...(isPackument ? { 'accept-encoding': 'identity' } : {}) },
    }, (upstreamRes) => {
      if (!isPackument || upstreamRes.statusCode !== 200) {
        res.writeHead(upstreamRes.statusCode!, upstreamRes.headers)
        upstreamRes.pipe(res)
        return
      }
      readBody(upstreamRes).then((body) => {
        respondJson(res, withoutVersion(JSON.parse(body) as Packument, version))
      }, (err: unknown) => {
        res.destroy(err as Error)
      })
    })
    req.pipe(upstream)
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/`,
    close: async () => new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    }),
  }
}

async function readBody (stream: http.IncomingMessage): Promise<string> {
  const chunks: Buffer[] = []
  for await (const chunk of stream) {
    chunks.push(chunk as Buffer)
  }
  return Buffer.concat(chunks).toString()
}

/** `packument` as a registry that never published `version` serves it. */
function withoutVersion (packument: Packument, version: string): Packument {
  delete packument.versions[version]
  delete packument.time?.[version]
  const highest = Object.keys(packument.versions).sort().at(-1)!
  for (const [tag, tagged] of Object.entries(packument['dist-tags'] ?? {})) {
    if (tagged === version) packument['dist-tags']![tag] = highest
  }
  return packument
}

function respondJson (res: http.ServerResponse, value: unknown): void {
  const body = JSON.stringify(value)
  res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) })
  res.end(body)
}

function readLockfile (): LockfileFile {
  return readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
}

function packageKeys (lockfile: LockfileFile, name: string): string[] {
  return Object.keys(lockfile.packages ?? {}).filter((key) => key.startsWith(`${name}@`)).sort()
}

function readRecordedRegistryRoutes (): Record<string, string> | undefined {
  const state = JSON.parse(fs.readFileSync('node_modules/.pnpm-workspace-state-v1.json', 'utf8'))
  return state.settings.registryRoutes
}

function routeScope (registry: string): void {
  writeYamlFileSync('pnpm-workspace.yaml', { registries: { '@pnpm.e2e': registry } })
}

// The child's locked 100.1.0 is not served by the registry the scope moves
// to, so the resolving install picks the version it serves, from the parent's
// range.
test('moving a scope re-resolves its plain-keyed packages from the new registry', async () => {
  prepare({ name: 'test-registry-routes', version: '1.0.0', dependencies: { [PARENT]: '100.0.0' } })
  routeScope(REGISTRY)
  await execPnpm(['install'])
  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual([`${CHILD}@${UNPUBLISHED}`])
  expect(readRecordedRegistryRoutes()?.['@pnpm.e2e']).toBe(REGISTRY)

  routeScope(other.url)
  await execPnpm(['install'])

  const lockfile = readLockfile()
  expect(packageKeys(lockfile, PARENT)).toStrictEqual([`${PARENT}@100.0.0`])
  expect(packageKeys(lockfile, CHILD)).toStrictEqual([`${CHILD}@100.0.0`])
  expect(readRecordedRegistryRoutes()?.['@pnpm.e2e']).toBe(other.url)
})

// A frozen install installs the lockfile as recorded, and leaves the record of
// where its packages came from for the resolving install that follows.
test('a frozen install keeps the moved packages and the recorded routes', async () => {
  prepare({ name: 'test-registry-routes', version: '1.0.0', dependencies: { [PARENT]: '100.0.0' } })
  routeScope(REGISTRY)
  await execPnpm(['install'])
  const written = fs.readFileSync(WANTED_LOCKFILE, 'utf8')

  routeScope(other.url)
  await execPnpm(['install', '--frozen-lockfile'])

  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).toBe(written)
  // The plain keys still come from the old registry.
  expect(readRecordedRegistryRoutes()?.['@pnpm.e2e']).toBe(REGISTRY)
})
