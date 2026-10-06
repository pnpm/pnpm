import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { ProjectManifest } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'

import { closeServer } from '../utils/closeServer.js'
import { testDefaults } from '../utils/index.js'

// A registry whose metadata disagrees with the package.json in the tarball
// it serves. The lockfile records the registry metadata, whatever the command
// and whether the store already holds the tarball.
// Covers https://github.com/pnpm/pnpm/issues/16615

// Its tarball marks peer-b and peer-c as optional peers.
const WITH_PEERS = '@pnpm.e2e/abc-optional-peers'
const MINIMUM_RELEASE_AGE = 5760
const REGISTRY_PEERS = {
  '@pnpm.e2e/peer-a': '^1.0.0',
  '@pnpm.e2e/peer-b': '^1.0.0',
  '@pnpm.e2e/peer-c': '^1.0.0',
}
const LOCKED_PEERS = {
  '@pnpm.e2e/peer-a': '1.0.0',
  '@pnpm.e2e/peer-b': '1.0.0',
  '@pnpm.e2e/peer-c': '1.0.0',
}

let server: http.Server
let registry: string

beforeAll(async () => {
  server = createPeerMetaDroppingRegistryProxy(WITH_PEERS)
  await new Promise<void>((resolve) => {
    server.listen(0, resolve)
  })
  registry = `http://localhost:${(server.address() as AddressInfo).port}/`
})

afterAll(async () => {
  await closeServer(server)
})

test.each(['1.0.0', '^1.0.0'])('every install command records the registry metadata of %s', async (specifier) => {
  const project = prepareEmpty()
  let manifest: ProjectManifest = {
    dependencies: {
      [WITH_PEERS]: specifier,
      '@pnpm.e2e/peer-a': '1.0.0',
      '@pnpm.e2e/peer-b': '1.0.0',
      '@pnpm.e2e/peer-c': '1.0.0',
    },
  }
  const withReleaseAge = (minimumReleaseAge: number, extra: Record<string, unknown> = {}) =>
    testDefaults({ ...extra, minimumReleaseAge, registriesByScope: { default: registry } })

  const assertRecordsRegistryMetadata = async (step: string): Promise<void> => {
    const { packages, snapshots } = project.readLockfile()
    const snapshotKey = Object.keys(snapshots).find((key) => key.startsWith(`${WITH_PEERS}@`))!
    expect({ step, package: packages[`${WITH_PEERS}@1.0.0`], snapshot: snapshots[snapshotKey] }).toStrictEqual({
      step,
      package: { resolution: expect.anything(), peerDependencies: REGISTRY_PEERS },
      snapshot: { dependencies: LOCKED_PEERS },
    })
    const lockfile = fs.readFileSync(WANTED_LOCKFILE, 'utf8')
    for (const minimumReleaseAge of [0, MINIMUM_RELEASE_AGE]) {
      // eslint-disable-next-line no-await-in-loop -- each dedupe reads the lockfile the previous one wrote
      await install(manifest, withReleaseAge(minimumReleaseAge, { dedupe: true }))
      expect({ step, minimumReleaseAge, lockfile: fs.readFileSync(WANTED_LOCKFILE, 'utf8') })
        .toStrictEqual({ step, minimumReleaseAge, lockfile })
    }
  }

  await install(manifest, withReleaseAge(MINIMUM_RELEASE_AGE))
  await assertRecordsRegistryMetadata('install')

  manifest = (await addDependenciesToPackage(manifest, ['@pnpm.e2e/foo@100.0.0'], withReleaseAge(0))).updatedManifest
  await assertRecordsRegistryMetadata('add without release age')

  await install(manifest, withReleaseAge(0, { dedupe: true }))
  await assertRecordsRegistryMetadata('dedupe without release age')

  await install(manifest, withReleaseAge(0))
  await assertRecordsRegistryMetadata('install without release age')

  await install(manifest, withReleaseAge(0, { force: true }))
  await assertRecordsRegistryMetadata('install --force')

  rimrafSync('node_modules')
  manifest = (await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], withReleaseAge(0))).updatedManifest
  await assertRecordsRegistryMetadata('add without node_modules')

  rimrafSync('node_modules')
  fs.rmSync(WANTED_LOCKFILE)
  await install(manifest, withReleaseAge(0))
  await assertRecordsRegistryMetadata('install without a lockfile over a warm store')
  project.storeHas(WITH_PEERS, '1.0.0')
})

function createPeerMetaDroppingRegistryProxy (pkgName: string): http.Server {
  return http.createServer((req, res) => {
    forwardDroppingPeerMeta(pkgName, req, res).catch((err: unknown) => {
      res.writeHead(500, { 'content-type': 'text/plain' })
      res.end(String(err))
    })
  })
}

async function forwardDroppingPeerMeta (pkgName: string, req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  const upstream = await fetch(`http://localhost:${REGISTRY_MOCK_PORT}${req.url}`, {
    method: req.method,
    headers: { accept: req.headers.accept ?? '*/*' },
  })
  const contentType = upstream.headers.get('content-type') ?? ''
  if (decodeURIComponent(req.url!) !== `/${pkgName}` || !contentType.includes('json')) {
    res.writeHead(upstream.status, { 'content-type': contentType })
    res.end(Buffer.from(await upstream.arrayBuffer()))
    return
  }
  const doc = await upstream.json() as { versions?: Record<string, Record<string, unknown>> }
  for (const versionMeta of Object.values(doc.versions ?? {})) {
    delete versionMeta.peerDependenciesMeta
  }
  res.writeHead(upstream.status, { 'content-type': 'application/json' })
  res.end(JSON.stringify(doc))
}
