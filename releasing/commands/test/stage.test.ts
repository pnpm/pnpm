import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { pack, stage } from '@pnpm/releasing.commands'
import tar from 'tar-stream'
import { temporaryDirectory } from 'tempy'

import { DEFAULT_OPTS } from './publish/utils/index.js'

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

describe('stage command', () => {
  test('stage publish posts to the staging endpoint and returns keyed JSON with stageId', async () => {
    const pkgName = '@scope/stage-publish-json'
    const stageId = 'f8e7a45b-7a5f-4f31-8e6d-9dd1c6ef38c0'
    prepare({ name: pkgName, version: '1.0.0' })

    const registry = await createRegistry(async (request) => {
      if (request.method === 'POST' && decodeURIComponent(request.url.pathname) === `/-/stage/package/${pkgName}`) {
        return { status: 201, body: { stageId } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'publish', '--json'] },
        dir: process.cwd(),
        json: true,
      }, ['publish'])

      expect(typeof result).toBe('object')
      const output = JSON.parse((result as { output: string }).output)
      expect(output[pkgName]).toMatchObject({
        name: pkgName,
        version: '1.0.0',
        stageId,
      })
      expect(registry.requests.some((request) => request.method === 'POST' && decodeURIComponent(request.url.pathname) === `/-/stage/package/${pkgName}`)).toBe(true)
    } finally {
      await registry.close()
    }
  })

  test('stage list and view fetch staged package metadata', async () => {
    const item = {
      id: STAGE_ID,
      packageName: '@scope/example-package',
      version: '1.2.3',
      tag: 'latest',
      createdAt: '2026-03-16T09:00:00.000Z',
      actor: 'user',
      actorType: 'user',
      shasum: '4f7f5f1d5bcf2f72f6e4d6c4f3b2812d8a2f6c19',
    }
    const registry = await createRegistry((request) => {
      if (request.method === 'GET' && request.url.pathname === '/-/stage') {
        expect(request.url.searchParams.get('page')).toBe('0')
        expect(request.url.searchParams.get('perPage')).toBe('100')
        const packageFilter = request.url.searchParams.get('package')
        if (packageFilter != null) {
          expect(packageFilter).toBe('@scope/example-package')
        }
        return { body: { items: [item], page: 0, perPage: 100, total: 1 } }
      }
      if (request.method === 'GET' && request.url.pathname === `/-/stage/${STAGE_ID}`) {
        return { body: item }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const listResult = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'list', '--json'] },
        json: true,
      }, ['list'])
      expect(JSON.parse(listResult as string)).toStrictEqual([item])

      const filteredListResult = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'list', '--json'] },
        json: true,
      }, ['list', '@scope/example-package'])
      expect(JSON.parse(filteredListResult as string)).toStrictEqual([item])

      const viewResult = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'view'] },
      }, ['view', STAGE_ID])
      expect(viewResult).toContain('package name: @scope/example-package')
      expect(viewResult).toContain('staged by: user (user)')
    } finally {
      await registry.close()
    }
  })

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

  test('stage download --json is keyed by package name and writes the staged tarball', async () => {
    const pkgName = '@scope/stage-download-json'
    prepare({ name: pkgName, version: '1.0.0' })
    const packDir = temporaryDirectory()
    const packResult = await pack.api({
      ...DEFAULT_OPTS,
      argv: { original: ['pack'] },
      dir: process.cwd(),
      packDestination: packDir,
    })
    const tarballData = fs.readFileSync(packResult.tarballPath)

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
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage', 'download', '--json'] },
        dir: downloadDir,
        json: true,
      }, ['download', STAGE_ID])

      const output = JSON.parse(result as string)
      expect(output[pkgName]).toMatchObject({
        name: pkgName,
        version: '1.0.0',
        filename: `scope-stage-download-json-1.0.0-${STAGE_ID}.tgz`,
      })
      expect(output.undefined).toBeUndefined()
      expect(fs.existsSync(path.join(downloadDir, `scope-stage-download-json-1.0.0-${STAGE_ID}.tgz`))).toBe(true)
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
