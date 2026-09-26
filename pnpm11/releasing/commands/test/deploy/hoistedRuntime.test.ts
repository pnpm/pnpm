import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createGzip } from 'node:zlib'

import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import { getLockfileImporterId, readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import { preparePackages } from '@pnpm/prepare'
import { deploy } from '@pnpm/releasing.commands'
import type { DepPath } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import tar from 'tar-stream'

import { DEFAULT_OPTS } from './utils/index.js'

const RUNTIME_VERSION = '22.0.0'
const PLATFORMS = [
  ['linux', 'x64'],
  ['linux', 'arm64'],
  ['darwin', 'x64'],
  ['darwin', 'arm64'],
  ['win32', 'x64'],
  ['win32', 'arm64'],
] as const

test.each([
  ['hoisted shared lockfile', 'hoisted', false],
  ['hoisted legacy install', 'hoisted', true],
  ['isolated shared lockfile', 'isolated', false],
] as const)('prod deploy materializes a downloaded engines.runtime only outside a hoisted node_modules (%s)', async (_label, nodeLinker, forceLegacyDeploy) => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        private: true,
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const lockfileDir = process.cwd()
  const projectDir = path.join(lockfileDir, 'project-1')
  const opts = {
    ...DEFAULT_OPTS,
    dir: lockfileDir,
    lockfileDir,
    workspaceDir: lockfileDir,
    sharedWorkspaceLockfile: true,
  }
  const selected = await filterProjectsBySelectorObjectsFromDir(lockfileDir, [{ namePattern: 'project-1' }])
  await install.handler({
    ...opts,
    allProjects: selected.allProjects,
    dev: true,
    production: true,
  })

  const { bytes, integrity } = await runtimeTarball()
  const server = await serveRuntimeTarball(bytes)
  try {
    const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
    expect(lockfile).toBeTruthy()
    const projectId = getLockfileImporterId(lockfileDir, projectDir)
    const importer = lockfile!.importers[projectId]
    importer.dependencies = {
      ...importer.dependencies,
      node: `runtime:${RUNTIME_VERSION}`,
    }
    importer.specifiers = {
      ...importer.specifiers,
      node: `runtime:${RUNTIME_VERSION}`,
    }
    const depPath = `node@runtime:${RUNTIME_VERSION}` as DepPath
    lockfile!.packages = {
      ...lockfile!.packages,
      [depPath]: {
        hasBin: true,
        version: RUNTIME_VERSION,
        resolution: {
          type: 'variations',
          variants: PLATFORMS.map(([os, cpu]) => ({
            targets: [{ os, cpu }],
            resolution: {
              type: 'binary' as const,
              archive: 'tarball' as const,
              bin: { node: 'bin/node' },
              integrity,
              url: server.url,
            },
          })),
        },
      },
    }
    await writeWantedLockfile(lockfileDir, lockfile!)

    const manifestPath = path.join(projectDir, 'package.json')
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
    manifest.engines = {
      runtime: { name: 'node', version: RUNTIME_VERSION, onFail: 'download' },
    }
    fs.writeFileSync(manifestPath, JSON.stringify(manifest))
    const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(lockfileDir, [{ namePattern: 'project-1' }])

    await deploy.handler({
      ...opts,
      allProjects,
      dev: false,
      forceLegacyDeploy,
      nodeLinker,
      production: true,
      recursive: true,
      selectedProjectsGraph,
    }, ['dist'])

    const nodeDir = path.join(lockfileDir, 'dist/node_modules/node')
    expect(fs.existsSync(path.join(lockfileDir, 'dist/node_modules/is-positive'))).toBe(true)
    if (nodeLinker === 'hoisted') {
      expect(fs.existsSync(nodeDir)).toBe(false)
      expect(fs.existsSync(path.join(lockfileDir, 'dist/node_modules/.bin/node'))).toBe(false)
    } else {
      expect(fs.existsSync(path.join(nodeDir, 'package.json'))).toBe(true)
      expect(fs.existsSync(path.join(nodeDir, 'bin/node'))).toBe(true)
    }
    const deployedManifest = JSON.parse(fs.readFileSync(path.join(lockfileDir, 'dist/package.json'), 'utf8'))
    expect(deployedManifest.engines.runtime).toEqual({
      name: 'node',
      onFail: 'download',
      version: RUNTIME_VERSION,
    })
    if (!forceLegacyDeploy) {
      const deployLockfile = fs.readFileSync(path.join(lockfileDir, 'dist/pnpm-lock.yaml'), 'utf8')
      expect(deployLockfile).toContain(`node@runtime:${RUNTIME_VERSION}`)
    }
  } finally {
    await server.close()
  }
})

async function runtimeTarball (): Promise<{ bytes: Buffer, integrity: string }> {
  const pack = tar.pack()
  pack.entry({ name: 'package/bin/node', mode: 0o755 }, '#!/bin/sh\n')
  const gzip = createGzip()
  const chunks: Buffer[] = []
  const done = new Promise<Buffer>((resolve, reject) => {
    gzip.on('data', (chunk: Buffer) => {
      chunks.push(chunk)
    })
    gzip.on('end', () => {
      resolve(Buffer.concat(chunks))
    })
    gzip.on('error', reject)
    pack.on('error', reject)
  })
  pack.pipe(gzip)
  pack.finalize()
  const bytes = await done
  return {
    bytes,
    integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}`,
  }
}

function serveRuntimeTarball (bytes: Buffer): Promise<{ url: string, close: () => Promise<void> }> {
  const server = http.createServer((_req, res) => {
    res.writeHead(200, {
      'content-length': bytes.length,
      'content-type': 'application/gzip',
    })
    res.end(bytes)
  })
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      if (address == null || typeof address === 'string') {
        reject(new Error('runtime tarball server has no port'))
        return
      }
      resolve({
        url: `http://127.0.0.1:${address.port}/node.tar.gz`,
        close: async () => {
          await new Promise<void>((done, fail) => {
            server.close(err => {
              if (err != null) fail(err)
              else done()
            })
          })
        },
      })
    })
  })
}
