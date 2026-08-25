import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepare, preparePackages, tempDir } from '@pnpm/prepare'
import { stage } from '@pnpm/releasing.commands'
import { REGISTRY_URL } from '@pnpm/testing.command-defaults'
import { getRegistryMockToken, REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import tar from 'tar-stream'
import { temporaryDirectory } from 'tempy'

import { DEFAULT_OPTS } from './publish/utils/index.js'

const STAGE_ID = '1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f'
const SECOND_STAGE_ID = '2b8f1c14-4a0d-4a4a-9a2e-6c5a2f0a1b33'
const THIRD_STAGE_ID = '3c7e9d25-5b1e-4b5b-8b3f-7d6b3a1b2c44'

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
      configByUri: configByUri(),
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
      configByUri: configByUri(),
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
    const restoreTty = forceNonInteractiveTty()
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        argv: { original: ['stage'] },
      }, ['approve', STAGE_ID])).rejects.toMatchObject({ code: 'ERR_PNPM_OTP_NON_INTERACTIVE' })
    } finally {
      restoreTty()
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

  test('stage approve reuses one one-time password across the batch and asks for a new one when it expires', async () => {
    const items = [
      { id: STAGE_ID, packageName: '@pnpmtest/stage-batch-a', version: '1.0.0' },
      { id: SECOND_STAGE_ID, packageName: '@pnpmtest/stage-batch-b', version: '1.0.0' },
      { id: THIRD_STAGE_ID, packageName: '@pnpmtest/stage-batch-c', version: '1.0.0' },
    ]
    const passwordsByStageId = new Map<string, Array<string | undefined>>()
    let baseUrl = ''
    let acceptedOtp = 'otp-1'
    const registry = await createRegistry((request) => {
      const describedStageIdOfRequest = describedStageId(request)
      if (describedStageIdOfRequest) {
        const item = items.find(({ id }) => id === describedStageIdOfRequest)
        return item ? { status: 200, body: item } : { status: 404, body: { error: 'not found' } }
      }
      if (request.method === 'GET' && request.url.pathname === '/-/v1/done') {
        return { status: 200, body: { token: acceptedOtp } }
      }
      const stageId = approvedStageId(request)
      if (stageId) {
        const otp = headerValue(request.headers['npm-otp'])
        passwordsByStageId.set(stageId, [...passwordsByStageId.get(stageId) ?? [], otp])
        if (otp === acceptedOtp) {
          // The password the first approval obtained expires right after it,
          // so the second approval has to obtain a new one.
          if (stageId === STAGE_ID) acceptedOtp = 'otp-2'
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
      return { status: 404, body: { error: 'not found' } }
    })
    baseUrl = registry.url
    const restoreTty = forceInteractiveTty()
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
      }, ['approve', STAGE_ID, SECOND_STAGE_ID, THIRD_STAGE_ID])
      expect(result).toStrictEqual({ exitCode: 0, output: 'Approved 3 staged packages successfully.' })
      expect(passwordsByStageId.get(STAGE_ID)).toEqual([undefined, 'otp-1'])
      expect(passwordsByStageId.get(SECOND_STAGE_ID)).toEqual(['otp-1', 'otp-2'])
      expect(passwordsByStageId.get(THIRD_STAGE_ID)).toEqual(['otp-2'])
    } finally {
      restoreTty()
      await registry.close()
    }
  }, 30000)

  test('stage approve keeps going after a staged package is rejected by the registry', async () => {
    const items = [
      { id: STAGE_ID, packageName: '@pnpmtest/stage-failing', version: '1.0.0' },
      { id: SECOND_STAGE_ID, packageName: '@pnpmtest/stage-passing', version: '1.0.0' },
    ]
    const registry = await createRegistry((request) => {
      const describedStageIdOfRequest = describedStageId(request)
      if (describedStageIdOfRequest) {
        const item = items.find(({ id }) => id === describedStageIdOfRequest)
        return item ? { status: 200, body: item } : { status: 404, body: { error: 'not found' } }
      }
      const stageId = approvedStageId(request)
      if (stageId === STAGE_ID) return { status: 409, body: { error: 'version already exists' } }
      if (stageId === SECOND_STAGE_ID) return { status: 201, body: { ok: true } }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        otp: '123456',
      }, ['approve', STAGE_ID, SECOND_STAGE_ID])
      expect(result).toStrictEqual({ exitCode: 1, output: 'Approved 1 of 2 staged packages.' })
    } finally {
      await registry.close()
    }
  })

  test('stage approve publishes workspace dependencies before their dependents', async () => {
    const workspaceDir = prepareWorkspaceWithDependency()
    const items = [
      { id: STAGE_ID, packageName: '@pnpmtest/stage-dependent', version: '1.0.0' },
      { id: SECOND_STAGE_ID, packageName: '@pnpmtest/stage-dependency', version: '1.0.0' },
    ]
    const approved: string[] = []
    const registry = await createRegistry((request) => {
      const describedStageIdOfRequest = describedStageId(request)
      if (describedStageIdOfRequest) {
        const item = items.find(({ id }) => id === describedStageIdOfRequest)
        return item ? { status: 200, body: item } : { status: 404, body: { error: 'not found' } }
      }
      const stageId = approvedStageId(request)
      if (stageId) {
        approved.push(stageId)
        return { status: 201, body: { ok: true } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        dir: workspaceDir,
        otp: '123456',
        workspaceDir,
        workspacePackagePatterns: ['packages/*'],
      }, ['approve', STAGE_ID, SECOND_STAGE_ID])
      expect(result).toStrictEqual({ exitCode: 0, output: 'Approved 2 staged packages successfully.' })
      expect(approved).toEqual([SECOND_STAGE_ID, STAGE_ID])
    } finally {
      await registry.close()
    }
  })

  test('stage approve skips a staged package whose workspace dependency could not be approved', async () => {
    const workspaceDir = prepareWorkspaceWithDependency()
    const items = [
      { id: STAGE_ID, packageName: '@pnpmtest/stage-dependent', version: '1.0.0' },
      { id: SECOND_STAGE_ID, packageName: '@pnpmtest/stage-dependency', version: '1.0.0' },
    ]
    const approveAttempts: string[] = []
    const registry = await createRegistry((request) => {
      const describedStageIdOfRequest = describedStageId(request)
      if (describedStageIdOfRequest) {
        const item = items.find(({ id }) => id === describedStageIdOfRequest)
        return item ? { status: 200, body: item } : { status: 404, body: { error: 'not found' } }
      }
      const stageId = approvedStageId(request)
      if (stageId) {
        approveAttempts.push(stageId)
        return { status: 409, body: { error: 'version already exists' } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        dir: workspaceDir,
        otp: '123456',
        workspaceDir,
        workspacePackagePatterns: ['packages/*'],
      }, ['approve', STAGE_ID, SECOND_STAGE_ID])
      expect(result).toStrictEqual({ exitCode: 1, output: 'Approved 0 of 2 staged packages.' })
      // The dependency is attempted (and retried by the registry client); the
      // dependent is never sent, as its dependency never reached the registry.
      expect(approveAttempts).not.toContain(STAGE_ID)
      expect(approveAttempts).toContain(SECOND_STAGE_ID)
    } finally {
      await registry.close()
    }
  })

  test('stage approve sends one request for a repeated stage id, whatever its spelling', async () => {
    const seen: string[] = []
    const registry = await createRegistry((request) => {
      const stageId = approvedStageId(request)
      if (stageId) {
        seen.push(stageId)
        return { status: 201, body: { ok: true } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        otp: '123456',
      }, ['approve', STAGE_ID, STAGE_ID.toUpperCase()])
      expect(result).toBe(`Staged package ${STAGE_ID} approved and published successfully.`)
      expect(seen).toEqual([STAGE_ID])
      expect(registry.requests.filter(({ method }) => method === 'GET')).toHaveLength(0)
    } finally {
      await registry.close()
    }
  })

  test('stage approve aborts the batch when a staged version cannot be read', async () => {
    const approveAttempts: string[] = []
    const registry = await createRegistry((request) => {
      if (describedStageId(request)) {
        return { status: 401, body: { error: 'unauthorized' } }
      }
      const stageId = approvedStageId(request)
      if (stageId) {
        approveAttempts.push(stageId)
        return { status: 201, body: { ok: true } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      await expect(stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        otp: '123456',
      }, ['approve', STAGE_ID, SECOND_STAGE_ID])).rejects.toMatchObject({ code: 'ERR_PNPM_STAGE_REGISTRY_ERROR' })
      expect(approveAttempts).toHaveLength(0)
    } finally {
      await registry.close()
    }
  })

  test('stage approve treats a package name the registry made up as no name at all', async () => {
    const items = [
      { id: STAGE_ID, packageName: '@pnpmtest/stage-dependent', version: '1.0.0' },
      // The dependency's name only looks like the workspace package's.
      { id: SECOND_STAGE_ID, packageName: '@pnpmtest/stage-dependency\u202E', version: '1.0.0' },
    ]
    const workspaceDir = prepareWorkspaceWithDependency()
    const approved: string[] = []
    const registry = await createRegistry((request) => {
      const describedStageIdOfRequest = describedStageId(request)
      if (describedStageIdOfRequest) {
        const item = items.find(({ id }) => id === describedStageIdOfRequest)
        return item ? { status: 200, body: item } : { status: 404, body: { error: 'not found' } }
      }
      const stageId = approvedStageId(request)
      if (stageId) {
        approved.push(stageId)
        return { status: 201, body: { ok: true } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    try {
      const result = await stage.handler({
        ...stageOpts(registry.url),
        cliOptions: { otp: '123456' },
        dir: workspaceDir,
        otp: '123456',
        workspaceDir,
        workspacePackagePatterns: ['packages/*'],
      }, ['approve', STAGE_ID, SECOND_STAGE_ID])
      expect(result).toStrictEqual({ exitCode: 0, output: 'Approved 2 staged packages successfully.' })
      // Without a workspace identity it sorts after the workspace packages
      // instead of ahead of the dependent it resembles.
      expect(approved).toEqual([STAGE_ID, SECOND_STAGE_ID])
    } finally {
      await registry.close()
    }
  })

  test('stage approve without a stage id requires an interactive terminal', async () => {
    const registry = await createRegistry(() => ({ status: 500, body: { error: 'nothing should be requested' } }))
    const restoreTty = forceNonInteractiveTty()
    try {
      await expect(stage.handler(stageOpts(registry.url), ['approve']))
        .rejects.toMatchObject({ code: 'ERR_PNPM_STAGE_ID_REQUIRED' })
      expect(registry.requests).toHaveLength(0)
    } finally {
      restoreTty()
      await registry.close()
    }
  })

  test('stage approve does not offer a staged version the registry listed without a stage id', async () => {
    const registry = await createRegistry((request) => {
      if (request.method === 'GET' && request.url.pathname === '/-/stage') {
        return {
          status: 200,
          body: {
            items: [{ id: '../../../-/npm/v1/tokens', packageName: '@pnpmtest/spoofed', version: '1.0.0' }],
            total: 1,
          },
        }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    const restoreTty = forceInteractiveTty()
    try {
      await expect(stage.handler(stageOpts(registry.url), ['approve']))
        .resolves.toBe('There are no staged packages awaiting approval.')
    } finally {
      restoreTty()
      await registry.close()
    }
  })

  test('stage approve without a stage id reports an empty staging area instead of prompting', async () => {
    const registry = await createRegistry((request) => {
      if (request.method === 'GET' && request.url.pathname === '/-/stage') {
        return { status: 200, body: { items: [], total: 0 } }
      }
      return { status: 404, body: { error: 'not found' } }
    })
    const restoreTty = forceInteractiveTty()
    try {
      await expect(stage.handler(stageOpts(registry.url), ['approve']))
        .resolves.toBe('There are no staged packages awaiting approval.')
    } finally {
      restoreTty()
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

/**
 * The stage id a `GET /-/stage/<id>` request describes, as `stage approve`
 * reads a named staged version's metadata.
 */
function describedStageId (request: RegistryRequest): string | undefined {
  if (request.method !== 'GET') return undefined
  const match = /^\/-\/stage\/([^/]+)$/.exec(request.url.pathname)
  return match?.[1]
}

function approvedStageId (request: RegistryRequest): string | undefined {
  if (request.method !== 'POST') return undefined
  const match = /^\/-\/stage\/([^/]+)\/approve$/.exec(request.url.pathname)
  return match?.[1]
}

/**
 * A workspace whose `@pnpmtest/stage-dependent` package depends on its
 * `@pnpmtest/stage-dependency` package. Returns the workspace root.
 */
function prepareWorkspaceWithDependency (): string {
  const workspaceDir = tempDir()
  preparePackages([
    {
      location: 'packages/dependency',
      package: { name: '@pnpmtest/stage-dependency', version: '1.0.0' },
    },
    {
      location: 'packages/dependent',
      package: {
        name: '@pnpmtest/stage-dependent',
        version: '1.0.0',
        dependencies: { '@pnpmtest/stage-dependency': 'workspace:*' },
      },
    },
  ], { tempDir: path.join(workspaceDir, 'project') })
  return workspaceDir
}

function configByUri (): Record<string, Record<string, { authToken: string }>> {
  return {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
      '@': { authToken: getRegistryMockToken() },
    },
  }
}

async function fetchPackument (pkgName: string): Promise<Response> {
  return fetch(`${REGISTRY_URL}/${pkgName.replaceAll('/', '%2F')}`, {
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
    registriesByScope: { default: registry },
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
  return overrideTty(true)
}

function forceNonInteractiveTty (): () => void {
  return overrideTty(false)
}

/**
 * Pins both terminal streams to `isTTY`, so a test exercises the interactive
 * or the non-interactive path whether or not the suite itself runs on a
 * terminal. Returns the restore function.
 */
function overrideTty (isTTY: boolean): () => void {
  const originalStdin = Object.getOwnPropertyDescriptor(process.stdin, 'isTTY')
  const originalStdout = Object.getOwnPropertyDescriptor(process.stdout, 'isTTY')
  Object.defineProperty(process.stdin, 'isTTY', { value: isTTY, configurable: true })
  Object.defineProperty(process.stdout, 'isTTY', { value: isTTY, configurable: true })
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
