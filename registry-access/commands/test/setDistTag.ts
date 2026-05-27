import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { SyntheticOtpError } from '@pnpm/network.web-auth'
import { setDistTag } from '@pnpm/registry-access.client'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

const REGISTRY_URL = 'https://registry.npmjs.org/'
const PUT_PATH = /^\/-\/package\/pnpm\/dist-tags\/latest-10$/

describe('setDistTag', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('sends npm-auth-type: web when authType=web and no OTP yet', async () => {
    let capturedHeaders: Record<string, string | string[] | undefined> = {}
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(({ headers }) => {
      capturedHeaders = headers as typeof capturedHeaders
      return { statusCode: 200, data: {} }
    })

    await setDistTag({
      packageName: 'pnpm',
      version: '10.34.0',
      distTag: 'latest-10',
      registryUrl: REGISTRY_URL,
      fetchFromRegistry: createFetchFromRegistry({}),
      authType: 'web',
    })

    expect(capturedHeaders['npm-auth-type']).toBe('web')
    expect(capturedHeaders['npm-otp']).toBeUndefined()
  })

  it('keeps npm-auth-type: web alongside npm-otp on the web-flow retry', async () => {
    let capturedHeaders: Record<string, string | string[] | undefined> = {}
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(({ headers }) => {
      capturedHeaders = headers as typeof capturedHeaders
      return { statusCode: 200, data: {} }
    })

    await setDistTag({
      packageName: 'pnpm',
      version: '10.34.0',
      distTag: 'latest-10',
      registryUrl: REGISTRY_URL,
      fetchFromRegistry: createFetchFromRegistry({}),
      authType: 'web',
      otp: 'a'.repeat(64),
    })

    expect(capturedHeaders['npm-auth-type']).toBe('web')
    expect(capturedHeaders['npm-otp']).toBe('a'.repeat(64))
  })

  it('sends authType=legacy and npm-otp when the user passed --otp', async () => {
    let capturedHeaders: Record<string, string | string[] | undefined> = {}
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(({ headers }) => {
      capturedHeaders = headers as typeof capturedHeaders
      return { statusCode: 200, data: {} }
    })

    await setDistTag({
      packageName: 'pnpm',
      version: '10.34.0',
      distTag: 'latest-10',
      registryUrl: REGISTRY_URL,
      fetchFromRegistry: createFetchFromRegistry({}),
      authType: 'legacy',
      otp: '123456',
    })

    expect(capturedHeaders['npm-auth-type']).toBe('legacy')
    expect(capturedHeaders['npm-otp']).toBe('123456')
  })

  it('throws SyntheticOtpError carrying authUrl/doneUrl when the 401 body has them', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(401, {
      authUrl: 'https://www.npmjs.com/login?next=/-/v1/done?sessionId=abc',
      doneUrl: 'https://registry.npmjs.org/-/v1/done?sessionId=abc',
    })

    let caught: unknown
    try {
      await setDistTag({
        packageName: 'pnpm',
        version: '10.34.0',
        distTag: 'latest-10',
        registryUrl: REGISTRY_URL,
        fetchFromRegistry: createFetchFromRegistry({}),
      })
    } catch (err) {
      caught = err
    }
    expect(caught).toBeInstanceOf(SyntheticOtpError)
    expect((caught as SyntheticOtpError).body).toEqual({
      authUrl: 'https://www.npmjs.com/login?next=/-/v1/done?sessionId=abc',
      doneUrl: 'https://registry.npmjs.org/-/v1/done?sessionId=abc',
    })
  })

  it('throws SyntheticOtpError without body when the 401 mentions "one-time pass"', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(
      401,
      '"You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."'
    )

    await expect(setDistTag({
      packageName: 'pnpm',
      version: '10.34.0',
      distTag: 'latest-10',
      registryUrl: REGISTRY_URL,
      fetchFromRegistry: createFetchFromRegistry({}),
    })).rejects.toBeInstanceOf(SyntheticOtpError)
  })

  it('throws UNAUTHORIZED PnpmError on a 401 that is not an OTP challenge', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: PUT_PATH,
    }).reply(401, 'Bad token')

    await expect(setDistTag({
      packageName: 'pnpm',
      version: '10.34.0',
      distTag: 'latest-10',
      registryUrl: REGISTRY_URL,
      fetchFromRegistry: createFetchFromRegistry({}),
    })).rejects.toMatchObject({ code: 'ERR_PNPM_UNAUTHORIZED' })
  })
})
