import { once } from 'node:events'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { publish } from '@pnpm/releasing.commands'

interface CapturedRequest {
  method: string
  url: string
  body: string
}

let server: http.Server
let captured: CapturedRequest[]
let registry: string

beforeEach(async () => {
  captured = []
  server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (chunk) => {
      body += chunk
    })
    req.on('end', () => {
      captured.push({ method: req.method!, url: req.url!, body })
      res.writeHead(201, { 'content-type': 'application/json' })
      res.end(JSON.stringify({ ok: true, success: true }))
    })
  })
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const { port } = server.address() as import('node:net').AddressInfo
  registry = `http://127.0.0.1:${port}/`
})

afterEach(async () => {
  server.close()
  await once(server, 'close')
})

test('publish sends the readme to the registry as metadata (embed-readme off)', async () => {
  prepare({ name: 'publish-readme-off', version: '1.0.0' })
  fs.writeFileSync('README.md', '# Hello\n')

  await runPublish(process.cwd(), false)

  expect(publishedVersionManifest().readme).toBe('# Hello\n')
})

test('publish embeds the readme in the version manifest when embed-readme is on', async () => {
  prepare({ name: 'publish-readme-on', version: '1.0.0' })
  fs.writeFileSync('README.md', '# Hello\n')

  await runPublish(process.cwd(), true)

  expect(publishedVersionManifest().readme).toBe('# Hello\n')
})

test('publish omits readme metadata when the package has no README', async () => {
  prepare({ name: 'publish-readme-none', version: '1.0.0' })
  fs.rmSync('README.md', { force: true })

  await runPublish(process.cwd(), false)

  expect(publishedVersionManifest().readme).toBeUndefined()
})

test('publish reads the readme from a pre-built tarball', async () => {
  prepare({ name: 'publish-readme-tarball', version: '1.0.0' })
  fs.writeFileSync('README.md', '# From tarball\n')

  const { pack } = await import('@pnpm/releasing.commands')
  const packResult = await pack.api({
    dir: process.cwd(),
    argv: { original: [] },
    embedReadme: false,
    catalogs: {},
    ignoreScripts: true,
  } as unknown as Parameters<typeof pack.api>[0])

  await runPublish(process.cwd(), false, [path.resolve(packResult.tarballPath)])

  expect(publishedVersionManifest().readme).toBe('# From tarball\n')
})

async function runPublish (dir: string, embedReadme: boolean, params: string[] = []): Promise<void> {
  await publish.publish({
    dir,
    argv: { original: [] },
    gitChecks: false,
    ignoreScripts: true,
    embedReadme,
    skipManifestObfuscation: false,
    catalogs: {},
    registries: { default: registry },
    configByUri: { [registry]: { '@//': { authToken: 'test' } } },
    tag: 'latest',
    userAgent: 'test',
    fetchRetries: 0,
    fetchRetryFactor: 1,
    fetchRetryMintimeout: 0,
    fetchRetryMaxtimeout: 1,
    fetchTimeout: 10000,
  } as unknown as Parameters<typeof publish.publish>[0], params)
}

function publishedVersionManifest (): Record<string, unknown> {
  const put = captured.find((request) => request.method === 'PUT')
  expect(put).toBeDefined()
  const document = JSON.parse(put!.body)
  const version = Object.keys(document.versions)[0]
  return document.versions[version]
}
