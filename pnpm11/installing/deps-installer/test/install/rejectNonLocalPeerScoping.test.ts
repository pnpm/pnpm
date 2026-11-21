import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import zlib from 'node:zlib'

import { expect, test } from '@jest/globals'
import { install, mutateModules } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

function createTarball (manifest: Record<string, unknown>): { bytes: Buffer, integrity: string } {
  const jsonBytes = Buffer.from(JSON.stringify(manifest), 'utf8')
  const indexBytes = Buffer.from("module.exports = 'pkg'\n", 'utf8')
  const blocks = [
    tarEntry('package/package.json', jsonBytes),
    tarEntry('package/index.js', indexBytes),
    Buffer.alloc(1024, 0),
  ]
  const bytes = zlib.gzipSync(Buffer.concat(blocks))
  const integrity = `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`
  return { bytes, integrity }
}

function tarEntry (name: string, content: Buffer): Buffer {
  const header = Buffer.alloc(512, 0)
  header.write(name, 0, Math.min(name.length, 100), 'utf8')
  header.write('0000644\0', 100, 8, 'ascii')
  header.write('0000000\0', 108, 8, 'ascii')
  header.write('0000000\0', 116, 8, 'ascii')
  header.write(`${content.length.toString(8).padStart(11, '0')}\0`, 124, 12, 'ascii')
  header.write(`${Math.floor(Date.now() / 1000).toString(8).padStart(11, '0')}\0`, 136, 12, 'ascii')
  header.fill(0x20, 148, 156)
  header[156] = '0'.charCodeAt(0)
  header.write('ustar\0', 257, 6, 'ascii')
  header.write('00', 263, 2, 'ascii')
  let sum = 0
  for (let idx = 0; idx < 512; idx++) sum += header[idx]
  header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'ascii')
  const padLen = (512 - (content.length % 512)) % 512
  return Buffer.concat([header, content, Buffer.alloc(padLen, 0)])
}

async function withRegistry (
  manifests: Array<Record<string, unknown>>,
  run: (registryUrl: string) => Promise<void>
): Promise<void> {
  const entries = new Map<string, { manifest: Record<string, unknown>, bytes: Buffer, integrity: string }>()
  for (const manifest of manifests) {
    const { bytes, integrity } = createTarball(manifest)
    entries.set(String(manifest.name), { manifest, bytes, integrity })
  }
  const server = http.createServer((req, res) => {
    const addr = server.address() as { port: number }
    const urlPath = (req.url ?? '').slice(1)
    if (urlPath.endsWith('-1.0.0.tgz')) {
      const hit = entries.get(urlPath.slice(0, -'-1.0.0.tgz'.length))
      if (hit != null) {
        res.writeHead(200, { 'content-type': 'application/octet-stream' })
        res.end(hit.bytes)
        return
      }
    }
    const hit = entries.get(urlPath)
    if (hit != null) {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: urlPath,
        'dist-tags': { latest: '1.0.0' },
        versions: {
          '1.0.0': {
            ...hit.manifest,
            dist: { tarball: `http://127.0.0.1:${addr.port}/${urlPath}-1.0.0.tgz`, integrity: hit.integrity },
          },
        },
      }))
      return
    }
    res.writeHead(404)
    res.end('not found')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const addr = server.address() as { port: number }
    await run(`http://127.0.0.1:${addr.port}/`)
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
}

test('switching a direct dependency from file: to registry stops authorizing its stale lockfile local peer', async () => {
  prepareEmpty()
  const peerDir = path.resolve('..', 'stale-peer')
  fs.mkdirSync(peerDir, { recursive: true })
  fs.writeFileSync(path.join(peerDir, 'package.json'), JSON.stringify({ name: 'stale-peer', version: '1.0.0' }))
  const peerSpec = `file:${peerDir.replace(/\\/g, '/')}`

  const localHelperDir = path.resolve('local-helper')
  fs.mkdirSync(localHelperDir, { recursive: true })
  fs.writeFileSync(path.join(localHelperDir, 'package.json'), JSON.stringify({
    name: 'helper',
    version: '1.0.0',
    peerDependencies: { 'stale-peer': peerSpec },
  }))

  await withRegistry([
    { name: 'helper', version: '1.0.0' },
    { name: 'evil-pkg', version: '1.0.0', peerDependencies: { 'stale-peer': peerSpec } },
  ], async (registryUrl) => {
    await install(
      { name: 'app', version: '1.0.0', dependencies: { helper: 'file:./local-helper', 'evil-pkg': '1.0.0' } },
      testDefaults({ autoInstallPeers: true, registriesByScope: { default: registryUrl } })
    )

    // Switch `helper` from `file:./local-helper` to registry `^1.0.0`: the stale lockfile `file:` entry
    // under the same alias `helper` must NOT authorize `evil-pkg`'s host-filesystem peer.
    await expect(install(
      { name: 'app', version: '1.0.0', dependencies: { helper: '^1.0.0', 'evil-pkg': '1.0.0' } },
      testDefaults({ autoInstallPeers: true, registriesByScope: { default: registryUrl } })
    )).rejects.toMatchObject({
      code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    })
  })
})

test('detached optional-peer candidate checks scope authorizations per declaring candidate across importer orders', async () => {
  for (const reverseOrder of [false, true]) {
    prepareEmpty()
    const peerDir = path.resolve('..', 'opt-peer-dir')
    fs.mkdirSync(peerDir, { recursive: true })
    fs.writeFileSync(path.join(peerDir, 'package.json'), JSON.stringify({ name: 'local-peer', version: '1.0.0' }))
    const peerSpec = `file:${peerDir.replace(/\\/g, '/')}`

    const impADir = path.resolve('packages/imp-a') as ProjectRootDir
    const impBDir = path.resolve('packages/imp-b') as ProjectRootDir
    fs.mkdirSync(impADir, { recursive: true })
    fs.mkdirSync(impBDir, { recursive: true })

    // eslint-disable-next-line no-await-in-loop -- isolated workspace run per importer order
    await withRegistry([
      {
        name: 'consumer-b',
        version: '1.0.0',
        peerDependencies: { 'evil-opt': '^1.0.0', 'trusted-opt': '^1.0.0' },
        peerDependenciesMeta: { 'evil-opt': { optional: true }, 'trusted-opt': { optional: true } },
      },
      { name: 'trusted-opt', version: '1.0.0' },
      { name: 'evil-opt', version: '1.0.0', peerDependencies: { 'local-peer': peerSpec } },
    ], async (registryUrl) => {
      const projA = {
        buildIndex: 0,
        manifest: { name: 'imp-a', version: '1.0.0', dependencies: { 'trusted-opt': '1.0.0', 'evil-opt': '1.0.0' } },
        rootDir: impADir,
      }
      const projB = {
        buildIndex: 1,
        manifest: { name: 'imp-b', version: '1.0.0', dependencies: { 'consumer-b': '1.0.0' } },
        rootDir: impBDir,
      }
      const ordered = reverseOrder ? [projB, projA] : [projA, projB]
      // `projA` fails when hoisting `evil-opt`'s required peer, and `projB` must never probe `local-peer` via `evil-opt`
      // regardless of whether `projA` or `projB` was ordered first.
      await expect(mutateModules(
        ordered.map(({ rootDir }) => ({ mutation: 'install' as const, rootDir })),
        testDefaults({
          allProjects: ordered,
          autoInstallPeers: true,
          dedupePeerDependents: true,
          packageExtensions: {
            'trusted-opt': { peerDependencies: { 'local-peer': peerSpec } },
          },
          registriesByScope: { default: registryUrl },
        })
      )).rejects.toMatchObject({
        code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
      })
    })
  }
})
