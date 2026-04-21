import { describe, expect, it } from '@jest/globals'
import { WebAuthTimeoutError } from '@pnpm/network.web-auth'

describe('WebAuthTimeoutError', () => {
  it('stores endTime, startTime, and timeout', () => {
    const err = new WebAuthTimeoutError(310_000, 10_000, 300_000)
    expect(err).toMatchObject({ endTime: 310_000, startTime: 10_000, timeout: 300_000 })
  })

  it('has ERR_PNPM_WEBAUTH_TIMEOUT code', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.code).toBe('ERR_PNPM_WEBAUTH_TIMEOUT')
  })

  it('includes a hint about re-running the command', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.hint).toMatch(/Re-run/)
  })

  it('has a descriptive message', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.message).toMatch(/timed out/)
  })
})
