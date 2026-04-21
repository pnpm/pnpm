import { promises as fs } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, it, jest } from '@jest/globals'
// First run downloads packages from registry-mock — slow on Windows CI
jest.setTimeout(600_000)
import { fetchFromPnpmRegistry } from '@pnpm/agent.client'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { StoreIndex } from '@pnpm/store.index'
import type { DepPath, ProjectId } from '@pnpm/types'
import { createRegistryServer } from 'pnpm-agent'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

describe('pnpm-agent integration', () => {
  let server: http.Server
  let serverPort: number
  let serverStoreDir: string
  let serverCacheDir: string

  beforeAll(async () => {
    // Create server store in a temp directory
    const tmpBase = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-test-server-'))
    serverStoreDir = path.join(tmpBase, 'store')
    serverCacheDir = path.join(tmpBase, 'cache')

    server = await createRegistryServer({
      storeDir: serverStoreDir,
      cacheDir: serverCacheDir,
      registries: { default: REGISTRY },
    })

    // Listen on random port
    await new Promise<void>((resolve) => {
      server.listen(0, resolve)
    })
    serverPort = (server.address() as any).port // eslint-disable-line @typescript-eslint/no-explicit-any
  })

  afterAll(async () => {
    const { finishWorkers } = await import('../../../worker/src/index.js')
    await finishWorkers()
    await new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err) {
          reject(err)
        } else {
          resolve()
        }
      })
    })
    await fs.rm(path.dirname(serverStoreDir), { recursive: true, force: true })
  })

  it('returns a lockfile with importers keyed by "."', async () => {
    const tmpClient = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-test-importers-'))
    const clientStoreDir = path.join(tmpClient, 'store')
    await fs.mkdir(clientStoreDir, { recursive: true })
    const clientStoreIndex = new StoreIndex(clientStoreDir)

    try {
      const result = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      // The lockfile must have importers keyed by "." (not by the server's temp dir)
      const importerKeys = Object.keys(result.lockfile.importers)
      expect(importerKeys).toEqual(['.'])

      // The "." importer must have specifiers and dependencies
      const rootImporter = result.lockfile.importers['.' as ProjectId]
      expect(rootImporter).toBeTruthy()
      expect(rootImporter.specifiers).toBeTruthy()
      expect(rootImporter.dependencies).toBeTruthy()
      expect(rootImporter.dependencies?.['is-positive']).toBeTruthy()
      await result.fileDownloads
    } finally {
      clientStoreIndex.close()
      await fs.rm(tmpClient, { recursive: true, force: true })
    }
  })

  it('resolves a single dependency and returns lockfile + files', async () => {
    // Create a client store in a temp directory
    const tmpClient = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-test-client-'))
    const clientStoreDir = path.join(tmpClient, 'store')
    await fs.mkdir(clientStoreDir, { recursive: true })
    const clientStoreIndex = new StoreIndex(clientStoreDir)

    try {
      const result = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      // Verify lockfile was returned
      expect(result.lockfile).toBeTruthy()
      expect(result.lockfile.lockfileVersion).toBeTruthy()

      // Verify packages were resolved
      const packages = result.lockfile.packages ?? {}
      const depPaths = Object.keys(packages)
      expect(depPaths.length).toBeGreaterThanOrEqual(1)

      // Verify at least one package has a resolution with integrity
      const hasIntegrity = depPaths.some(dp => {
        const pkg = packages[dp as DepPath]
        return pkg?.resolution && typeof pkg.resolution === 'object' && 'integrity' in pkg.resolution
      })
      expect(hasIntegrity).toBe(true)

      // Verify stats
      expect(result.stats.totalPackages).toBeGreaterThanOrEqual(1)
      await result.fileDownloads
    } finally {
      clientStoreIndex.close()
      await fs.rm(tmpClient, { recursive: true, force: true })
    }
  })

  it('returns consistent lockfile on repeated requests', async () => {
    const tmpClient = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-test-client2-'))
    const clientStoreDir = path.join(tmpClient, 'store')
    await fs.mkdir(clientStoreDir, { recursive: true })
    const clientStoreIndex = new StoreIndex(clientStoreDir)

    try {
      const result1 = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      const result2 = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      // Same dependency → same lockfile
      expect(Object.keys(result1.lockfile.packages ?? {})).toEqual(
        Object.keys(result2.lockfile.packages ?? {})
      )

      // Wait for file downloads to complete before cleanup
      await result1.fileDownloads
      await result2.fileDownloads
    } finally {
      clientStoreIndex.close()
      await fs.rm(tmpClient, { recursive: true, force: true })
    }
  })

  it('returns 404 for unknown endpoints', async () => {
    const result = await new Promise<{ statusCode: number, body: string }>((resolve, reject) => {
      const req = http.request(`http://localhost:${serverPort}/v1/unknown`, {
        method: 'POST',
      }, (res) => {
        const chunks: Buffer[] = []
        res.on('data', (chunk: Buffer) => chunks.push(chunk))
        res.on('end', () => resolve({
          statusCode: res.statusCode!,
          body: Buffer.concat(chunks).toString(),
        }))
      })
      req.on('error', reject)
      req.end()
    })

    expect(result.statusCode).toBe(404)
  })
})
