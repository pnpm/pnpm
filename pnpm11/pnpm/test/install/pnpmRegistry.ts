import fs from 'node:fs'
import http from 'node:http'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { loadJsonFileSync } from 'load-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// The pnpr server started by the test harness (see the with-registry jest
// preset) serves the resolver endpoint (/-/pnpr/v0/resolve) on the
// registry-mock port, so it doubles as the pnpr server under test.
const PNPR = `http://localhost:${REGISTRY_MOCK_PORT}`

let server: http.Server
let serverPort: number
let requestCount: number

beforeAll(async () => {
  // Counting proxy — forwards to the pnpr server and counts /-/pnpr/v0/resolve
  // requests so we can assert that the pnpr server path was actually taken.
  requestCount = 0
  server = http.createServer((req, res) => {
    if (!req.url) {
      res.writeHead(400).end()
      return
    }
    if (req.url === '/-/pnpr/v0/resolve') {
      requestCount++
    }
    const proxyReq = http.request(`${PNPR}${req.url}`, {
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
})

function configurePnprAuth (): void {
  const token = process.env.REGISTRY_MOCK_TOKEN
  if (!token) throw new Error('REGISTRY_MOCK_TOKEN is required for pnpr integration tests')
  fs.appendFileSync('.npmrc', `//localhost:${serverPort}/:_authToken=${token}\n`)
}

function prepareProject (manifest: Parameters<typeof prepare>[0]): void {
  prepare(manifest)
  configurePnprAuth()
}

function prepareWorkspace (packages: Parameters<typeof preparePackages>[0]): void {
  preparePackages(packages)
  configurePnprAuth()
}

test('pnpm install uses pnpr server when configured', async () => {
  prepareProject({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['install', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  // Verify the registry server received at least one request
  expect(requestCount).toBeGreaterThanOrEqual(1)

  // Verify the lockfile was created
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Verify the package was installed
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
})

test('pnpm install resolves optionalDependencies via the pnpr server', async () => {
  prepareProject({
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'is-negative': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['install', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  // The optional dependency must be forwarded to the server and resolved,
  // not silently dropped from the request.
  expect(fs.existsSync('node_modules/is-negative')).toBe(true)
})

test('a second resolution forwards the existing lockfile to the pnpr server', async () => {
  prepareProject({})

  // First add creates the lockfile.
  await execPnpm(['add', 'is-positive@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`])
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Second add reads that lockfile and forwards it to the pnpr server for
  // incremental resolution — exercises the on-disk lockfile being sent
  // over the wire without an in-memory round-trip.
  requestCount = 0
  await execPnpm(['add', 'is-negative@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`])

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(true)
})

test('pnpm add uses pnpr server when configured', async () => {
  prepareProject({
    dependencies: {
      'is-negative': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`]
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

test('pnpm remove uses pnpr server when configured', async () => {
  prepareProject({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  })

  requestCount = 0

  await execPnpm(
    ['remove', 'is-negative', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(false)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  expect(manifest.dependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-negative']).toBeUndefined()
})

test('pnpm add without a version uses the pnpr server and writes the save-prefix spec from the lockfile', async () => {
  prepareProject({})

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  // The pnpr server resolves "latest" to a concrete version and writes the
  // resolved version into the lockfile importer's `dependencies` map; the
  // client computes the save-prefix spec from that version.
  expect(manifest.dependencies?.['is-positive']).toMatch(/^\^\d+\.\d+\.\d+$/)
})

test('pnpm add -D uses pnpr server and targets devDependencies', async () => {
  prepareProject({})

  requestCount = 0

  await execPnpm(
    ['add', '-D', 'is-positive@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`]
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

test('pnpm add with multiple selectors uses pnpr server', async () => {
  prepareProject({})

  requestCount = 0

  await execPnpm(
    ['add', 'is-positive@1.0.0', 'is-negative@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  expect(requestCount).toBeGreaterThanOrEqual(1)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-negative')).toBe(true)

  const manifest = loadJsonFileSync<{ dependencies?: Record<string, string> }>('package.json')
  expect(manifest.dependencies?.['is-positive']).toBe('1.0.0')
  expect(manifest.dependencies?.['is-negative']).toBe('1.0.0')
})

test('pnpm --filter remove inside a workspace uses pnpr server', async () => {
  prepareWorkspace([
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
    ['--filter=project-b', 'remove', 'is-negative', `--config.pnprServer=http://localhost:${serverPort}`]
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

test('pnpm add inside a workspace project uses pnpr server', async () => {
  prepareWorkspace([
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
    ['--filter=project-b', 'add', 'is-negative@1.0.0', `--config.pnprServer=http://localhost:${serverPort}`]
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

test('pnpm install with pnpr server works in a workspace with multiple projects', async () => {
  prepareWorkspace([
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
    ['install', `--config.pnprServer=http://localhost:${serverPort}`]
  )

  // Verify the pnpr server was used
  expect(requestCount).toBeGreaterThanOrEqual(1)

  // Verify the lockfile was created
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  // Verify packages were installed in both projects
  expect(fs.existsSync('project-a/node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('project-b/node_modules/is-negative')).toBe(true)
})
