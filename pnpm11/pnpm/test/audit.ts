import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

/**
 * A registry stub that records the package names of every bulk advisory
 * request and answers that none of them is vulnerable. It runs in the test
 * process, so pnpm has to be spawned asynchronously for it to answer.
 */
let server: http.Server
let registry: string
let auditedPackageNames: string[][] = []

beforeAll(async () => {
  server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (chunk) => {
      body += chunk
    })
    req.on('end', () => {
      if (req.url === '/-/npm/v1/security/advisories/bulk') {
        auditedPackageNames.push(Object.keys(JSON.parse(body) as Record<string, string[]>).sort())
      }
      res.setHeader('content-type', 'application/json')
      res.end('{}')
    })
  })
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  registry = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`
})

afterAll(async () => {
  await new Promise<void>((resolve, reject) => {
    server.close((err) => {
      if (err) reject(err)
      else resolve()
    })
  })
})

function prepareAuditWorkspace (): void {
  preparePackages([
    { location: '.', package: { name: 'root', private: true, dependencies: { lodash: '1.0.0' } } },
    { location: 'packages/a', package: { name: 'pkg-a', version: '1.0.0', dependencies: { minimist: '1.2.0' } } },
    { location: 'packages/b', package: { name: 'pkg-b', version: '1.0.0', dependencies: { ms: '2.0.0' } } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  fs.writeFileSync('pnpm-lock.yaml', `lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      lodash:
        specifier: 1.0.0
        version: 1.0.0

  packages/a:
    dependencies:
      minimist:
        specifier: 1.2.0
        version: 1.2.0

  packages/b:
    dependencies:
      ms:
        specifier: 2.0.0
        version: 2.0.0

snapshots:

  lodash@1.0.0: {}

  minimist@1.2.0: {}

  ms@2.0.0: {}
`)
  auditedPackageNames = []
}

async function audit (args: string[], cwd: string): Promise<string[][]> {
  const env = { pnpm_config_registry: registry }
  const workspaceDir = process.cwd()
  process.chdir(cwd)
  try {
    await execPnpm(['audit', ...args], { env })
  } finally {
    process.chdir(workspaceDir)
  }
  return auditedPackageNames
}

test('pnpm audit run from a workspace project audits only that project', async () => {
  prepareAuditWorkspace()
  expect(await audit([], 'packages/a')).toStrictEqual([['minimist']])
})

test('pnpm audit run from the workspace root audits every project', async () => {
  prepareAuditWorkspace()
  expect(await audit([], '.')).toStrictEqual([['lodash', 'minimist', 'ms']])
})

test('pnpm -r audit run from a workspace project audits every project', async () => {
  prepareAuditWorkspace()
  expect(await audit(['--recursive'], 'packages/a')).toStrictEqual([['lodash', 'minimist', 'ms']])
})

test('pnpm audit --filter run from a workspace project audits the selected project', async () => {
  prepareAuditWorkspace()
  expect(await audit(['--filter', 'pkg-b'], 'packages/a')).toStrictEqual([['ms']])
})
