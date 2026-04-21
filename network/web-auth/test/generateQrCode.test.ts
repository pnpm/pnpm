import { describe, expect, it } from '@jest/globals'
import { generateQrCode } from '@pnpm/network.web-auth'

describe('generateQrCode', () => {
  it('returns a non-empty string', () => {
    const qr = generateQrCode('https://example.com')
    expect(qr).toEqual(expect.any(String))
    expect(qr.length).toBeGreaterThan(0)
  })

  it('produces different output for different inputs', () => {
    const qr1 = generateQrCode('https://example.com/a')
    const qr2 = generateQrCode('https://example.com/b')
    expect(qr1).not.toBe(qr2)
  })
})
