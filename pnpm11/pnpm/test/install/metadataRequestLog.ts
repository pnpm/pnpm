import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// The cases and their metadata request logs are shared with pnpm v12, see the
// table's `description`.
const CASE_TABLE = path.join(import.meta.dirname, '../../../../pnpr/.fixtures/request-logs/metadata-three-installs.json')

interface CaseTable {
  dependencies: Record<string, string>
  cases: Array<{
    name: string
    settings: Record<string, string | number>
    args: string[]
    installs: string[][]
  }>
}

interface RecordedRequest {
  method: string
  path: string
  accept?: string
  ifNoneMatch?: string
  ifModifiedSince?: string
  status: number
  etag?: string
}

interface RecordingRegistry {
  url: string
  takeRequests: () => RecordedRequest[]
  close: () => Promise<void>
}

const table: CaseTable = JSON.parse(fs.readFileSync(CASE_TABLE, 'utf8'))

let registry: RecordingRegistry | undefined

afterEach(async () => {
  await registry?.close()
  registry = undefined
})

test.each(table.cases)('$name', async ({ settings, args, installs: expected }) => {
  registry = await startRecordingRegistry()
  prepare({ dependencies: table.dependencies })
  writeYamlFileSync('pnpm-workspace.yaml', { registry: registry.url })
  // The test environment sets `minimumReleaseAge` as an environment
  // variable, which outranks pnpm-workspace.yaml.
  const env = Object.fromEntries(Object.entries(settings).map(([key, value]) => [
    `pnpm_config_${key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)}`,
    String(value),
  ]))
  const installs: RecordedRequest[][] = []
  for (let install = 0; install < 3; install++) {
    // Remove what the last install wrote, so this one resolves again.
    fs.rmSync('node_modules', { recursive: true, force: true })
    fs.rmSync('pnpm-lock.yaml', { force: true })
    // eslint-disable-next-line no-await-in-loop -- the installs share one cache and must run in order
    await execPnpm(['install', ...args], { env })
    installs.push(registry.takeRequests())
  }

  const etags = new Map(installs.flat()
    .filter((request) => request.status === 200 && request.etag != null)
    .map((request) => [`${request.path} ${documentOf(request)}`, request.etag]))
  expect(installs.map((requests) => requests.map((request) => describe(request, etags)).sort())).toStrictEqual(expected)
})

function documentOf (request: RecordedRequest): string {
  return request.accept?.startsWith('application/vnd.npm.install-v1+json') ? 'abbreviated' : 'full'
}

// A request sending some other ETag reads `other-etag`, which no case expects.
function describe (request: RecordedRequest, etags: Map<string, string | undefined>): string {
  const document = documentOf(request)
  let validators = ''
  if (request.ifNoneMatch != null) {
    validators += etags.get(`${request.path} ${document}`) === request.ifNoneMatch ? ' etag' : ' other-etag'
  }
  if (request.ifModifiedSince != null) {
    validators += ' since'
  }
  return `${request.method} ${request.path} ${document}${validators} ${request.status}`
}

// Serves the mock registry's npm endpoints at its root, so the recorded paths
// match the shared table. Tarball URLs in the packuments name the mock
// registry itself, so only metadata requests pass through here.
async function startRecordingRegistry (): Promise<RecordingRegistry> {
  let requests: RecordedRequest[] = []
  const server = http.createServer((req, res) => {
    const upstream = http.request({
      host: 'localhost',
      port: REGISTRY_MOCK_PORT,
      method: req.method,
      path: `/npm${req.url}`,
      headers: req.headers,
    }, (upstreamRes) => {
      requests.push({
        method: req.method!,
        path: req.url!,
        accept: req.headers.accept,
        ifNoneMatch: req.headers['if-none-match'],
        ifModifiedSince: req.headers['if-modified-since'],
        status: upstreamRes.statusCode!,
        etag: upstreamRes.headers.etag,
      })
      res.writeHead(upstreamRes.statusCode!, upstreamRes.headers)
      upstreamRes.pipe(res)
    })
    req.pipe(upstream)
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/`,
    takeRequests: () => {
      const taken = requests
      requests = []
      return taken
    },
    close: async () => new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    }),
  }
}
