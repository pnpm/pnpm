import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { createRegistryServer } from '@pnpm/agent.server'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

let server: http.Server
let serverPort: number
let serverStoreDir: string
let requestCount: number

beforeAll(async () => {
  const tmpBase = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-e2e-server-'))
  serverStoreDir = path.join(tmpBase, 'store')

  const realServer = await createRegistryServer({
    storeDir: serverStoreDir,
    cacheDir: path.join(tmpBase, 'cache'),
    registries: { default: REGISTRY },
  })

  await new Promise<void>((resolve) => {
    realServer.listen(0, resolve)
  })
  const realPort = (realServer.address() as { port: number }).port

  // Counting proxy — wraps the real server and counts /v1/install requests
  requestCount = 0
  server = http.createServer((req, res) => {
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
    req.pipe(proxyReq)
  })

  await new Promise<void>((resolve) => {
    server.listen(0, resolve)
  })
  serverPort = (server.address() as { port: number }).port
})

afterAll(() => {
  server.close()
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
