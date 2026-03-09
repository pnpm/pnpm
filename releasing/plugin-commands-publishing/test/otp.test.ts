import {
  type OtpContext,
  type OtpPublishResponse,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  OtpWebAuthTimeoutError,
  publishWithOtpHandling,
} from '../src/otp.js'

const MANIFEST = { name: 'test-pkg', version: '1.0.0' }
const TARBALL = Buffer.from('tarball-data')
const OK_RESPONSE: OtpPublishResponse = { ok: true, status: 200, statusText: 'OK', text: async () => '' }

function createOtpError (body?: { authUrl?: string; doneUrl?: string }): Error & { code: string; body?: typeof body } {
  const err = Object.assign(new Error('OTP required'), { code: 'EOTP', body })
  return err
}

function createContext (overrides: Partial<OtpContext> = {}): OtpContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb: () => void) => cb(),
    enquirer: { prompt: async () => ({ otp: '123456' }) },
    fetch: async () => ({ ok: false, json: async () => ({}) }),
    globalInfo: () => {},
    process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
    publish: async () => OK_RESPONSE,
    ...overrides,
  }
}

describe('publishWithOtpHandling', () => {
  test('returns response on successful publish', async () => {
    const context = createContext()
    const result = await publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })
    expect(result).toBe(OK_RESPONSE)
  })

  test('prompts for OTP on classic EOTP error', async () => {
    let callCount = 0
    const context = createContext({
      publish: async (_m, _t, opts) => {
        if (callCount++ === 0) throw createOtpError()
        expect(opts.otp).toBe('123456')
        return OK_RESPONSE
      },
    })
    const result = await publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })
    expect(result).toBe(OK_RESPONSE)
  })

  test('throws OtpNonInteractiveError when not a TTY', async () => {
    const context = createContext({
      process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
      publish: async () => { throw createOtpError() },
    })
    await expect(publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })).rejects.toThrow(OtpNonInteractiveError)
  })

  test('throws OtpSecondChallengeError on double EOTP', async () => {
    const context = createContext({
      publish: async () => { throw createOtpError() },
    })
    await expect(publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })).rejects.toThrow(OtpSecondChallengeError)
  })

  test('handles webauth flow and prints QR code', async () => {
    const messages: string[] = []
    let time = 0
    let fetchCount = 0
    const context = createContext({
      Date: { now: () => time },
      setTimeout: (cb: () => void) => { time += 1000; cb() },
      fetch: async () => {
        fetchCount++
        if (fetchCount < 2) return { ok: false, json: async () => ({}) }
        return { ok: true, json: async () => ({ done: true, token: 'web-token-123' }) }
      },
      globalInfo: (msg: string) => { messages.push(msg) },
      publish: async (_m, _t, opts) => {
        if (!opts.otp) throw createOtpError({ authUrl: 'https://registry.example.com/auth', doneUrl: 'https://registry.example.com/done' })
        expect(opts.otp).toBe('web-token-123')
        return OK_RESPONSE
      },
    })
    const result = await publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })
    expect(result).toBe(OK_RESPONSE)
    expect(messages).toHaveLength(1)
    expect(messages[0]).toContain('https://registry.example.com/auth')
    expect(messages[0]).toContain('Authenticate your account at:')
    // QR code generates unicode block characters
    expect(messages[0]).toContain('\u2588')
  })

  test('throws OtpWebAuthTimeoutError on webauth timeout', async () => {
    let time = 0
    const context = createContext({
      Date: { now: () => time },
      setTimeout: (cb: () => void) => { time += 6 * 60 * 1000; cb() },
      fetch: async () => ({ ok: false, json: async () => ({}) }),
      globalInfo: () => {},
      publish: async () => { throw createOtpError({ authUrl: 'https://example.com/auth', doneUrl: 'https://example.com/done' }) },
    })
    await expect(publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })).rejects.toThrow(OtpWebAuthTimeoutError)
  })

  test('re-throws non-OTP errors', async () => {
    const context = createContext({
      publish: async () => { throw new Error('network failure') },
    })
    await expect(publishWithOtpHandling({
      context,
      manifest: MANIFEST,
      publishOptions: {} as any,
      tarballData: TARBALL,
    })).rejects.toThrow('network failure')
  })
})
