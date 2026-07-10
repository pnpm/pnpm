import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { stage } from '@pnpm/releasing.commands'
import { REGISTRY_URL } from '@pnpm/testing.command-defaults'
import { getRegistryMockToken, REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import tar from 'tar-stream'
import { temporaryDirectory } from 'tempy'

import { DEFAULT_OPTS } from './publish/utils/index.js'

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    '@': { authToken: getRegistryMockToken() },
  },
}

const STAGE_ID = '1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f'

interface RegistryRequest {
  body: Buffer
  headers: http.IncomingHttpHeaders
  method: string
  url: URL
}

interface RegistryResponse {
  body?: Buffer | Record<string, unknown> | string
  headers?: Record<string, string>
  status?: number
}

type RegistryHandler = (request: RegistryRequest) => Promise<RegistryResponse> | RegistryResponse

describe('stage command against the registry mock', () => {
  // These tests run the staging lifecycle end-to-end against the pnpr
  // instance the with-registry jest preset boots; the ad-hoc mock registry
  // below is kept only for faults a well-behaved registry cannot produce.

  test('stage publish holds the package back until it is approved', async () => {
    const pkgName = '@pnpmtest/stage-e2e-lifecycle'
    prepare({ name: pkgName, version: '1.0.0' })
    const opts = {
      ...stageOpts(REGISTRY_URL),
      configByUri: CONFIG_BY_URI,
    }

    const publishResult = await stage.handler({
      ...opts,
      argv: { original: ['stage', 'publish', '--json'] },
      dir: process.cwd(),
      json: true,
    }, ['publish'])
    const published = JSON.parse((publishResult as { output: string }).output)
    expect(published[pkgName]).toMatchObject({ name: pkgName, version: '1.0.0' })
    const stageId = published[pkgName].stageId as string
    expect(typeof stageId).toBe('string')

    // Held back: the package is not installable before approval.
    expect((await fetchPackument(pkgName)).status).toBe(404)

    const listResult = await stage.handler({
      ...opts,
      argv: { original: ['stage', 'list', '--json'] },
      json: true,
    }, ['list', pkgName])
    const listed = JSON.parse(listResult as string)
    expect(listed).toHaveLength(1)
    expect(listed[0]).toMatchObject({
      id: stageId,
      packageName: pkgName,
      version: '1.0.0',
      tag: 'latest',
      actor: REGISTRY_MOCK_CREDENTIALS.username,
      actorType: 'user',
    })

    const viewResult = await stage.handler({
      ...opts,
      argv: { original: ['stage', 'view'] },
    }, ['view', stageId])
    expect(viewResult).toContain(`package name: ${pkgName}`)
    expect(viewResult).toContain(`staged by: ${REGISTRY_MOCK_CREDENTIALS.username} (user)`)

    const downloadDir = temporaryDirectory()
    const downloadResult = await stage.handler({
      ...opts,
      argv: { original: ['stage', 'download', '--json'] },
      dir: downloadDir,
      json: true,
    }, ['download', stageId])
    const downloaded = JSON.parse(downloadResult as string)
    const expectedFilename = `pnpmtest-stage-e2e-lifecycle-1.0.0-${stageId}.tgz`
    expect(downloaded[pkgName]).toMatchObject({ name: pkgName, version: '1.0.0', filename: expectedFilename })
    expect(fs.existsSync(path.join(downloadDir, expectedFilename))).toBe(true)

    await expect(stage.handler({
      ...opts,
      argv: { original: ['stage', 'approve'] },
    }, ['approve', stageId]))
      .resolves.toBe(`Staged package ${stageId} approved and published successfully.`)

    const packument = await fetchPackument(pkgName)
    expect(packument.status).toBe(200)
    expect((await packument.json() as { versions: Record<string, unknown> }).versions['1.0.0']).toBeTruthy()
    await expect(stage.handler({
      ...opts,
      argv: { original: ['stage', 'list'] },
    }, ['list', pkgName]))
      .resolves.toBe(`No staged versions of package name "${pkgName}".`)
  })

  test('stage reject deletes the staged publish without publishing it', async () => {
    const pkgName = '@pnpmtest/stage-e2e-rejected'
    prepare({ name: pkgName, version: '1.0.0' })
    const opts = {
      ...stageOpts(REGISTRY_URL),
      configByUri: CONFIG_BY_URI,
    }

    const publishResult = await stage.handler({
      ...opts,
      argv: { original: ['stage', 'publish'] },
      dir: process.cwd(),
    }, ['publish'])
    const output = (publishResult as { output: string }).output
    const stageId = /\(staged with id ([0-9a-f-]{36})\)/.exec(output)?.[1]
    if (!stageId) throw new Error(`staged line must carry the id: ${output}`)

    await expect(stage.handler({
      ...opts,
      argv: { original: ['stage', 'reject'] },
    }, ['reject', stageId]))
      .resolves.toBe(`Staged package ${stageId} has been rejected.`)

    expect((await fetchPackument(pkgName)).status).toBe(404)
    await expect(stage.handler({
      ...opts,
      argv: { original: ['stage', 'view'] },
    }, ['view', stageId])).rejects.toMatchObject({ code: 'ERR_PNPM_STAGE_REGISTRY_ERROR' })
    await expect(stage.handler({
      ...opts,
      argv: { original: ['stage', 'list'] },
    }, ['list', pkgName]))
      .resolves.toBe(`No staged versions of package name "${pkgName}".`)
  })

  test('stage list stops paginating at the fail-safe page cap', async () => {
    const fullPage = Array.from({ length: 100 }, () => ({ packageName: 'pkg', version: '1.0.0' }))
    const registry = await createRegistry(() => ({ body: { items: fullPage, total: 10_000_000 } }))
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'list', '--json'] },
        json: true,
      }, ['list'])

      expect(registry.requests).toHaveLength(1000)
      expect(JSON.parse(result as string)).toHaveLength(100_000)
    } finally {
      await registry.close()
    }
  }, 60_000)

  test('stage list rejects version specifiers', async () => {
    await expect(stage.handler(stageOpts('http://localhost:4873/'), ['list', 'pkg@1.0.0']))
      .rejects.toThrow('Version specifiers are not supported for listing staged packages')
  })

  test('stage list uses package-scoped auth for package filters', async () => {
    const registry = await createRegistry((request) => {
      expect(headerValue(request.headers.authorization)).toBe('Bearer scoped-token')
      return { body: { items: [], page: 0, perPage: 100, total: 0 } }
    })
    try {
      const registryUrl = new URL(registry.url)
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'list'] },
        configByUri: {
          [`//${registryUrl.host}/`]: {
            '@': { authToken: 'default-token' },
            '@scope': { authToken: 'scoped-token' },
          },
        },
      }, ['list', '@scope/example-package'])

      expect(result).toBe('No staged versions of package name "@scope/example-package".')
    } finally {
      await registry.close()
    }
  })

  test('stage approve and reject send configured OTP', async () => {
    const seen: Array<{ authType: string | undefined, method: string, npmCommand: string | undefined, otp: string | undefined, pathname: string }> = []
    const registry = await createRegistry((request) => {
      seen.push({
        authType: headerValue(request.headers['npm-auth-type']),
        method: request.method,
        npmCommand: headerValue(request.headers['npm-command']),
        otp: headerValue(request.headers['npm-otp']),
        pathname: request.url.pathname,
      })
      if (request.headers['npm-auth-type'] !== 'web') {
        return { status: 400, body: { error: 'missing web auth header' } }
      }
      if (request.headers['npm-command'] !== 'stage') {
        return { status: 400, body: { error: 'missing npm command header' } }
      }
      if (request.headers['npm-otp'] !== '123456') {
        return { status: 400, body: { error: 'missing otp' } }
      }
      if (request.method === 'POST' && request.url.pathname === `/-/stage/${STAGE_ID}/approve`) {
        return { status: 201, body: { ok: true } }
      }
      if (request.method === 'DELETE' && request.url.pathname === `/-/stage/${STAGE_ID}`) {
        return { status: 204, body: '' }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const opts = {
        ...stageOpts(registry.url),
        argv: { original: ['stage'] },
        cliOptions: { otp: '123456' },
        otp: '123456',
      }
      await expect(stage.handler(opts, ['approve', STAGE_ID]))
        .resolves.toBe(`Staged package ${STAGE_ID} approved and published successfully.`)
      await expect(stage.handler(opts, ['reject', STAGE_ID]))
        .resolves.toBe(`Staged package ${STAGE_ID} has been rejected.`)
      expect(seen).toEqual([
        { authType: 'web', method: 'POST', npmCommand: 'stage', otp: '123456', pathname: `/-/stage/${STAGE_ID}/approve` },
        { authType: 'web', method: 'DELETE', npmCommand: 'stage', otp: '123456', pathname: `/-/stage/${STAGE_ID}` },
      ])
    } finally {
      await registry.close()
    }
  })

  test('stage approve enters the web-auth OTP flow when the registry responds 401 with authUrl/doneUrl', async () => {
    const registry = await createRegistry(() => ({
      status: 401,
      body: {
        authUrl: 'https://www.npmjs.com/auth/cli/test-auth-id',
        doneUrl: 'https://registry.example.com/-/v1/done?authId=test-auth-id',
      },
    }))
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage'] },
      }, ['approve', STAGE_ID])).rejects.toMatchObject({ code: 'ERR_PNPM_OTP_NON_INTERACTIVE' })
    } finally {
      await registry.close()
    }
  })

  test('stage approve completes via the web-auth polling flow when the registry returns a token', async () => {
    const webOtpToken = 'web-auth-token-xyz'
    let baseUrl = ''
    const approveCalls: Array<string | undefined> = []
    const registry = await createRegistry((request) => {
      if (request.method === 'POST' && request.url.pathname === `/-/stage/${STAGE_ID}/approve`) {
        const otp = headerValue(request.headers['npm-otp'])
        approveCalls.push(otp)
        if (otp === webOtpToken) {
          return { status: 201, body: { ok: true } }
        }
        return {
          status: 401,
          body: {
            authUrl: 'http://example.invalid/auth-redirect',
            doneUrl: new URL('/-/v1/done?authId=test', baseUrl).href,
          },
        }
      }
      if (request.method === 'GET' && request.url.pathname === '/-/v1/done') {
        return { status: 200, body: { token: webOtpToken } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    baseUrl = registry.url
    const restoreTty = forceInteractiveTty()
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage'] },
      }, ['approve', STAGE_ID])
      expect(result).toBe(`Staged package ${STAGE_ID} approved and published successfully.`)
      expect(approveCalls).toEqual([undefined, webOtpToken])
    } finally {
      restoreTty()
      await registry.close()
    }
  }, 15000)

  test('stage approve surfaces 401 without web-auth or otp signals as a registry error', async () => {
    const registry = await createRegistry(() => ({
      status: 401,
      body: { error: 'unauthorized' },
      headers: { 'www-authenticate': 'Basic realm="example"' },
    }))
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage'] },
      }, ['approve', STAGE_ID])).rejects.toMatchObject({ code: 'ERR_PNPM_STAGE_REGISTRY_ERROR' })
    } finally {
      await registry.close()
    }
  })

  test('stage download rejects traversal through tarball manifest version', async () => {
    const outsideBase = `stage-download-outside-version-${process.pid}-${Date.now()}`
    const tarballData = await createPackageTarball({
      name: '@scope/stage-download-version',
      version: `1.0.0/../../${outsideBase}`,
    })
    const registry = await createRegistry((request) => {
      if (request.method === 'GET' && request.url.pathname === `/-/stage/${STAGE_ID}/tarball`) {
        return {
          body: tarballData,
          headers: { 'content-type': 'application/octet-stream' },
        }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    const downloadDir = temporaryDirectory()
    const outsidePath = path.resolve(downloadDir, '..', `${outsideBase}-${STAGE_ID}.tgz`)
    await fs.promises.rm(outsidePath, { force: true })
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'download'] },
        dir: downloadDir,
      }, ['download', STAGE_ID])).rejects.toMatchObject({
        code: 'ERR_PNPM_INVALID_PACKAGE_VERSION',
      })

      expect(fs.existsSync(outsidePath)).toBe(false)
      expect(fs.readdirSync(downloadDir)).toStrictEqual([])
    } finally {
      await fs.promises.rm(outsidePath, { force: true })
      await registry.close()
    }
  })

  test('stage download rejects traversal through tarball manifest package name', async () => {
    const outsideBase = `stage-download-outside-name-${process.pid}-${Date.now()}`
    const tarballData = await createPackageTarball({
      name: `@scope/../../${outsideBase}`,
      version: '1.0.0',
    })
    const registry = await createRegistry((request) => {
      if (request.method === 'GET' && request.url.pathname === `/-/stage/${STAGE_ID}/tarball`) {
        return {
          body: tarballData,
          headers: { 'content-type': 'application/octet-stream' },
        }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    const downloadDir = temporaryDirectory()
    const outsidePath = path.resolve(downloadDir, '..', `${outsideBase}-1.0.0-${STAGE_ID}.tgz`)
    await fs.promises.rm(outsidePath, { force: true })
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'download'] },
        dir: downloadDir,
      }, ['download', STAGE_ID])).rejects.toMatchObject({
        code: 'ERR_PNPM_INVALID_PACKAGE_NAME',
      })

      expect(fs.existsSync(outsidePath)).toBe(false)
      expect(fs.readdirSync(downloadDir)).toStrictEqual([])
    } finally {
      await fs.promises.rm(outsidePath, { force: true })
      await registry.close()
    }
  })

  test('stage publish --dry-run reports that packages would be staged', async () => {
    const pkgName = '@scope/stage-publish-dry-run'
    prepare({ name: pkgName, version: '1.0.0' })

    const registry = await createRegistry(() => ({ status: 500, body: { error: 'dry run should not upload' } }))
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'publish', '--dry-run'] },
        dir: process.cwd(),
        dryRun: true,
      }, ['publish'])

      expect(result).toStrictEqual({
        exitCode: 0,
        output: `+ ${pkgName}@1.0.0 (would stage)`,
      })
      expect(registry.requests).toHaveLength(0)
    } finally {
      await registry.close()
    }
  })
})

async function fetchPackument (pkgName: string): Promise<Response> {
  return fetch(`${REGISTRY_URL}/${pkgName.replace('/', '%2f')}`, {
    headers: { authorization: `Bearer ${getRegistryMockToken()}` },
  })
}

function stageOpts (registry: string): Parameters<typeof stage.handler>[0] {
  return {
    ...DEFAULT_OPTS,
    argv: { original: ['stage'] },
    configByUri: {},
    dir: process.cwd(),
    gitChecks: false,
    registries: { default: registry },
    registry,
  } as Parameters<typeof stage.handler>[0]
}

async function createRegistry (handler: RegistryHandler): Promise<{ close: () => Promise<void>, requests: RegistryRequest[], url: string }> {
  const requests: RegistryRequest[] = []
  const server = http.createServer(async (req, res) => {
    const body = await readRequestBody(req)
    const request = {
      body,
      headers: req.headers,
      method: req.method ?? 'GET',
      url: new URL(req.url ?? '/', `http://${req.headers.host}`),
    }
    requests.push(request)
    try {
      const response = await handler(request)
      writeResponse(res, response)
    } catch (error: unknown) {
      writeResponse(res, { status: 500, body: String(error) })
    }
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Registry server did not start')
  return {
    close: () => new Promise<void>((resolve, reject) => server.close((err) => err ? reject(err) : resolve())),
    requests,
    url: `http://127.0.0.1:${address.port}/`,
  }
}

function createPackageTarball (manifest: { name: string, version: string }): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const pack = tar.pack()
    const chunks: Buffer[] = []
    pack.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)))
    pack.on('error', reject)
    pack.on('end', () => resolve(Buffer.concat(chunks)))
    pack.entry({ name: 'package/package.json' }, JSON.stringify(manifest), (err?: Error | null) => {
      if (err) {
        reject(err)
        return
      }
      pack.finalize()
    })
  })
}

function headerValue (value: http.IncomingHttpHeaders[string]): string | undefined {
  return Array.isArray(value) ? value[0] : value
}

function forceInteractiveTty (): () => void {
  const originalStdin = Object.getOwnPropertyDescriptor(process.stdin, 'isTTY')
  const originalStdout = Object.getOwnPropertyDescriptor(process.stdout, 'isTTY')
  Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
  Object.defineProperty(process.stdout, 'isTTY', { value: true, configurable: true })
  return () => {
    if (originalStdin) {
      Object.defineProperty(process.stdin, 'isTTY', originalStdin)
    } else {
      delete (process.stdin as { isTTY?: boolean }).isTTY
    }
    if (originalStdout) {
      Object.defineProperty(process.stdout, 'isTTY', originalStdout)
    } else {
      delete (process.stdout as { isTTY?: boolean }).isTTY
    }
  }
}

function readRequestBody (req: http.IncomingMessage): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk) => chunks.push(Buffer.from(chunk)))
    req.on('end', () => resolve(Buffer.concat(chunks)))
    req.on('error', reject)
  })
}

function writeResponse (res: http.ServerResponse, response: RegistryResponse): void {
  const status = response.status ?? 200
  const headers = { ...response.headers }
  let body: Buffer | string
  if (Buffer.isBuffer(response.body)) {
    body = response.body
  } else if (typeof response.body === 'object' && response.body != null) {
    headers['content-type'] ??= 'application/json'
    body = JSON.stringify(response.body)
  } else {
    body = response.body ?? ''
  }
  res.writeHead(status, headers)
  res.end(body)
}
