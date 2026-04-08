import { promises as fs } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import { fetchFromPnpmRegistry } from '@pnpm/registry.client'
import { createRegistryServer } from '@pnpm/registry.server'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { StoreIndex } from '@pnpm/store.index'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

describe('pnpm-registry integration', () => {
  let server: http.Server
  let serverPort: number
  let serverStoreDir: string
  let serverCacheDir: string

  beforeAll(async () => {
    // Create server store in a temp directory
    const tmpBase = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-registry-test-server-'))
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

  it('resolves a single dependency and returns lockfile + files', async () => {
    // Create a client store in a temp directory
    const tmpClient = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-registry-test-client-'))
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
        const pkg = packages[dp]
        return pkg?.resolution && typeof pkg.resolution === 'object' && 'integrity' in pkg.resolution
      })
      expect(hasIntegrity).toBe(true)

      // Verify stats
      expect(result.stats.totalPackages).toBeGreaterThanOrEqual(1)
      expect(result.stats.filesToDownload).toBeGreaterThanOrEqual(1)

      // Verify files were written to client CAFS
      const storeFiles = await fs.readdir(path.join(clientStoreDir, 'files'), { recursive: true }).catch(() => [])
      expect(storeFiles.length).toBeGreaterThan(0)

      // Verify store index was updated
      let entryCount = 0
      for (const _ of clientStoreIndex.entries()) {
        entryCount++
      }
      expect(entryCount).toBeGreaterThanOrEqual(1)
    } finally {
      clientStoreIndex.close()
      await fs.rm(tmpClient, { recursive: true, force: true })
    }
  })

  it('deduplicates files on second install with warm store', async () => {
    const tmpClient = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-registry-test-client2-'))
    const clientStoreDir = path.join(tmpClient, 'store')
    await fs.mkdir(clientStoreDir, { recursive: true })
    const clientStoreIndex = new StoreIndex(clientStoreDir)

    try {
      // First install — all files are new
      const result1 = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      const filesDownloaded1 = result1.stats.filesToDownload
      expect(filesDownloaded1).toBeGreaterThan(0)

      // Second install with same dependency — all files should be cached
      const result2 = await fetchFromPnpmRegistry({
        registryUrl: `http://localhost:${serverPort}`,
        storeDir: clientStoreDir,
        storeIndex: clientStoreIndex,
        dependencies: {
          'is-positive': '1.0.0',
        },
      })

      // Client sent its store integrities, so server should report everything cached
      expect(result2.stats.alreadyInStore).toBeGreaterThanOrEqual(1)
      expect(result2.stats.filesToDownload).toBe(0)
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
