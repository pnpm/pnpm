import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { loadJsonFileSync } from 'load-json-file'
import { createRegistryServer } from 'pnpm-agent'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

let server: http.Server
let realServer: http.Server
let tmpBaseDir: string
let serverPort: number
let serverStoreDir: string
let requestCount: number

beforeAll(async () => {
  tmpBaseDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-e2e-server-'))
  serverStoreDir = path.join(tmpBaseDir, 'store')

  realServer = await createRegistryServer({
    storeDir: serverStoreDir,
    cacheDir: path.join(tmpBaseDir, 'cache'),
    registries: { default: REGISTRY },
  })

  await new Promise<void>((resolve) => {
    realServer.listen(0, resolve)
  })
  const realPort = (realServer.address() as { port: number }).port

  // Counting proxy — wraps the real server and counts /v1/install requests
  requestCount = 0
  server = http.createServer((req, res) => {
    if (!req.url) {
      res.writeHead(400).end()
      return
    }
    if (req.url === '/v1/install') {
      requestCount++
    }
    const proxyReq = http.request(`http://localhost:${realPort}${req.url}`, {
      method: req.method,
      headers: req.headers,
    }, (proxyRes) => {
      res.writeHead(proxyRes.statusCode!, proxyRes.headers)
      proxyRes.pipe(res)
    })
    proxyReq.on('error', (err) => {
      if (!res.headersSent) res.writeHead(502, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: err.message }))
    })
    req.pipe(proxyReq)
  })

  await new Promise<void>((resolve) => {
    server.listen(0, resolve)
  })
  serverPort = (server.address() as { port: number }).port
})

afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()))
  await new Promise<void>((resolve) => realServer.close(() => resolve()))
  await fs.promises.rm(tmpBaseDir, { recursive: true, force: true })
})

test('pnpm install uses pnpm agent when configured', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['install', `--config.agent=http://localhost:${serverPort}`]
  )

  // Verify the registry server received at least one request
  expect(requestCount).toBeGreaterThanOrEqual(1)

  // Verify the lockfile was created
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Verify the package was installed
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
})

test('pnpm add uses pnpm agent when configured', async () => {
  prepare({
    dependencies: {
      'is-negative': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive@1.0.0', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Both the new dep and the original dep should be installed
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(true)

  // Manifest should record the new dep
  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  expect(manifest.dependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-negative']).toBe('1.0.0')
})

test('pnpm remove uses pnpm agent when configured', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['remove', 'is-negative', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(false)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  expect(manifest.dependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-negative']).toBeUndefined()
})

test('pnpm add without a version uses the pnpm agent and writes the save-prefix spec from the lockfile', async () => {
  prepare({})

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  // The agent resolves "latest" to a concrete version and writes the
  // resolved version into the lockfile importer's `dependencies` map; the
  // client computes the save-prefix spec from that version.
  expect(manifest.dependencies?.['is-positive']).toMatch(/^\^\d+\.\d+\.\d+$/)
})

test('pnpm add -D uses pnpm agent and targets devDependencies', async () => {
  prepare({})

  requestCount = 0

  await execPnpm(
    ['add', '-D', 'is-positive@1.0.0', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)

  const manifest = loadJsonFileSync<{
    dependencies?: Record<string, string>
    devDependencies?: Record<string, string>
  }>('package.json')
  expect(manifest.devDependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-positive']).toBeUndefined()
})

test('pnpm add with multiple selectors uses pnpm agent', async () => {
  prepare({})

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive@1.0.0', 'is-negative@1.0.0', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(true)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  expect(manifest.dependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-negative']).toBe('1.0.0')
})

test('pnpm --filter remove inside a workspace uses pnpm agent', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'is-negative': '1.0.0',
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  requestCount = 0

  await execPnpm(
    ['--filter=project-b', 'remove', 'is-negative', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // project-b no longer has is-negative; project-a is unaffected
  expect(fs.existsSync('project-b/node_modules/is-negative')).toBe(false)
  expect(fs.existsSync('project-b/node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('project-a/node_modules/is-positive')).toBe(true)

  const projectBManifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('project-b/package.json')
  expect(projectBManifest.dependencies?.['is-negative']).toBeUndefined()
  expect(projectBManifest.dependencies?.['is-positive']).toBe('1.0.0')
})

test('pnpm add inside a workspace project uses pnpm agent', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  requestCount = 0

  await execPnpm(
    ['--filter=project-b', 'add', 'is-negative@1.0.0', `--config.agent=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // The newly added dep should be installed in project-b
  expect(fs.existsSync('project-b/node_modules/is-negative')).toBe(true)
  // The original dep in project-a should still be installed
  expect(fs.existsSync('project-a/node_modules/is-positive')).toBe(true)

  const projectBManifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('project-b/package.json')
  expect(projectBManifest.dependencies?.['is-negative']).toBe('1.0.0')
})

test('pnpm install with agent works in a workspace with multiple projects', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  requestCount = 0

  await execPnpm(
    ['install', `--config.agent=http://localhost:${serverPort}`]
  )

  // Verify the agent server was used
  expect(requestCount).toBeGreaterThanOrEqual(1)

  // Verify the lockfile was created
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Verify packages were installed in both projects
  expect(fs.existsSync('project-a/node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('project-b/node_modules/is-negative')).toBe(true)
})
