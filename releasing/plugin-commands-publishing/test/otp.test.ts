import { describe, expect, jest, test } from '@jest/globals'
import {
  type OtpContext,
  type OtpPublishResponse,
  type OtpWebAuthFetchOptions,
  type OtpWebAuthFetchResponse,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  OtpWebAuthTimeoutError,
  publishWithOtpHandling,
} from '../src/otp.js'

const FAKE_MANIFEST = {
  name: 'test-pkg',
  version: '1.0.0',
}

const FAKE_TARBALL = Buffer.from('fake-tarball')

const FAKE_PUBLISH_OPTIONS = {
  registry: 'https://registry.npmjs.org/',
}

function makeOkResponse (): OtpPublishResponse {
  return {
    ok: true,
    status: 200,
    statusText: 'OK',
    text: async () => '',
  }
}

function makeOtpError (extra: object = {}): Error & object {
  return Object.assign(new Error('OTP required'), {
    code: 'EOTP',
    ...extra,
  })
}

function makeContext (overrides: Partial<OtpContext> = {}): OtpContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb, _ms) => cb(),
    enquirer: {
      prompt: jest.fn<OtpContext['enquirer']['prompt']>().mockResolvedValue(undefined),
    },
    fetch: jest.fn<(url: string, opts: OtpWebAuthFetchOptions) => Promise<OtpWebAuthFetchResponse>>(),
    globalInfo: jest.fn(),
    process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
    publish: jest.fn<OtpContext['publish']>().mockResolvedValue(makeOkResponse()),
    ...overrides,
  }
}

describe('publishWithOtpHandling', () => {
  test('returns response immediately when publish succeeds', async () => {
    const context = makeContext()
    const response = await publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })
    expect(response.ok).toBe(true)
  })

  test('throws OtpNonInteractiveError when terminal is not a TTY', async () => {
    const context = makeContext({
      process: { stdin: { isTTY: false }, stdout: { isTTY: false } },
      publish: jest.fn<OtpContext['publish']>().mockRejectedValue(makeOtpError()),
    })
    await expect(publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })).rejects.toThrow(OtpNonInteractiveError)
  })

  test('prompts for OTP when classic OTP error (no authUrl/doneUrl, no npm-notice)', async () => {
    const enquirerPrompt = jest.fn<OtpContext['enquirer']['prompt']>().mockResolvedValue({ otp: '123456' })
    const publish = jest.fn<OtpContext['publish']>()
      .mockRejectedValueOnce(makeOtpError())
      .mockResolvedValueOnce(makeOkResponse())

    const context = makeContext({ enquirer: { prompt: enquirerPrompt }, publish })

    const response = await publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })

    expect(response.ok).toBe(true)
    expect(enquirerPrompt).toHaveBeenCalledTimes(1)
    expect(publish).toHaveBeenCalledTimes(2)
    expect(publish).toHaveBeenLastCalledWith(FAKE_MANIFEST, FAKE_TARBALL, { ...FAKE_PUBLISH_OPTIONS, otp: '123456' })
  })

  test('throws OtpSecondChallengeError when registry asks for OTP a second time', async () => {
    const enquirerPrompt = jest.fn<OtpContext['enquirer']['prompt']>().mockResolvedValue({ otp: '123456' })
    const publish = jest.fn<OtpContext['publish']>()
      .mockRejectedValueOnce(makeOtpError())
      .mockRejectedValueOnce(makeOtpError())

    const context = makeContext({ enquirer: { prompt: enquirerPrompt }, publish })

    await expect(publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })).rejects.toThrow(OtpSecondChallengeError)
  })

  test('uses webauth OTP flow when error body has authUrl and doneUrl', async () => {
    let callCount = 0
    const fakeDoneUrl = 'https://registry.npmjs.org/-/v1/login/poll/abc123'
    const fakeToken = 'webauth-token-xyz'

    const fetch = jest.fn<OtpContext['fetch']>().mockResolvedValue({
      ok: true,
      json: async () => ({ done: true, token: fakeToken }),
    })

    const publish = jest.fn<OtpContext['publish']>()
      .mockImplementationOnce(() => {
        callCount++
        return Promise.reject(makeOtpError({
          body: { authUrl: 'https://www.npmjs.com/login/abc123', doneUrl: fakeDoneUrl },
        }))
      })
      .mockImplementationOnce((_manifest, _tarball, opts) => {
        callCount++
        expect(opts.otp).toBe(fakeToken)
        return Promise.resolve(makeOkResponse())
      })

    const globalInfo = jest.fn()
    const context = makeContext({ fetch, publish, globalInfo, Date: { now: () => 0 } })

    const response = await publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })

    expect(response.ok).toBe(true)
    expect(callCount).toBe(2)
    expect(fetch).toHaveBeenCalledWith(fakeDoneUrl, expect.objectContaining({ method: 'GET' }))
    expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://www.npmjs.com/login/abc123'))
  })

  test('uses npm-notice webauth flow when error has npm-notice header with URL', async () => {
    const fakeToken = 'npm-notice-token-xyz'
    const noticeUrl = 'https://www.npmjs.com/login/ab12-cd34-ef56'
    const expectedPollUrl = 'https://registry.npmjs.org/-/v1/login/poll/ab12-cd34-ef56'
    const npmNoticeMsg = `Open ${noticeUrl} to use your security key for authentication`

    const fetch = jest.fn<OtpContext['fetch']>().mockResolvedValue({
      ok: true,
      json: async () => ({ done: true, token: fakeToken }),
    })

    const publish = jest.fn<OtpContext['publish']>()
      .mockRejectedValueOnce(makeOtpError({
        headers: {
          'www-authenticate': ['OTP'],
          'npm-notice': [npmNoticeMsg],
        },
      }))
      .mockImplementationOnce((_manifest, _tarball, opts) => {
        expect(opts.otp).toBe(fakeToken)
        return Promise.resolve(makeOkResponse())
      })

    const globalInfo = jest.fn()
    const context = makeContext({ fetch, publish, globalInfo, Date: { now: () => 0 } })

    const response = await publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })

    expect(response.ok).toBe(true)
    expect(fetch).toHaveBeenCalledWith(expectedPollUrl, expect.objectContaining({ method: 'GET' }))
    // The npm-notice message and QR code should be logged
    expect(globalInfo).toHaveBeenCalledTimes(1)
    const loggedMessage = (globalInfo as jest.Mock).mock.calls[0][0] as string
    expect(loggedMessage).toContain(npmNoticeMsg)
    // QR code content should also be in the message
    expect(loggedMessage).toContain(noticeUrl)
  })

  test('falls back to OTP prompt when npm-notice URL cannot be parsed for polling', async () => {
    const enquirerPrompt = jest.fn<OtpContext['enquirer']['prompt']>().mockResolvedValue({ otp: '654321' })
    const publish = jest.fn<OtpContext['publish']>()
      .mockRejectedValueOnce(makeOtpError({
        headers: {
          'npm-notice': ['No URL in this notice'],
        },
      }))
      .mockResolvedValueOnce(makeOkResponse())

    const context = makeContext({ enquirer: { prompt: enquirerPrompt }, publish })

    const response = await publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })

    expect(response.ok).toBe(true)
    expect(enquirerPrompt).toHaveBeenCalledTimes(1)
  })

  test('throws OtpWebAuthTimeoutError when webauth polling times out', async () => {
    let time = 0
    const dateNow = jest.fn(() => time)

    const fetch = jest.fn<OtpContext['fetch']>().mockResolvedValue({
      ok: true,
      json: async () => ({ done: false }),
    })

    const setTimeout = jest.fn<OtpContext['setTimeout']>().mockImplementation((cb, _ms) => {
      time += 1000
      cb()
    })

    const publish = jest.fn<OtpContext['publish']>()
      .mockRejectedValueOnce(makeOtpError({
        body: { authUrl: 'https://www.npmjs.com/login/abc123', doneUrl: 'https://registry.npmjs.org/-/v1/login/poll/abc123' },
      }))

    const context = makeContext({
      fetch,
      publish,
      Date: { now: dateNow },
      setTimeout,
    })

    await expect(publishWithOtpHandling({
      context,
      manifest: FAKE_MANIFEST,
      publishOptions: FAKE_PUBLISH_OPTIONS,
      tarballData: FAKE_TARBALL,
    })).rejects.toThrow(OtpWebAuthTimeoutError)
  })
})
