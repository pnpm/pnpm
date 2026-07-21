import { describe, expect, it } from '@jest/globals'
import { formatAuthUrlMessage } from '@pnpm/network.web-auth'

describe('formatAuthUrlMessage', () => {
  it('appends a QR code when one can be generated', () => {
    const message = formatAuthUrlMessage('https://example.com/auth', msg => {
      throw new Error(`Unexpected call to globalWarn: ${msg}`)
    })
    expect(message).toMatch(/^Authenticate your account at:\nhttps:\/\/example\.com\/auth\n\n/)
    expect(message.length).toBeGreaterThan('Authenticate your account at:\nhttps://example.com/auth\n\n'.length)
  })

  it('warns and falls back to a URL-only message when QR generation fails', () => {
    // Longer than the 2953-byte maximum QR data capacity (version 40 at
    // error-correction level L), which makes qrcode-terminal throw.
    const longAuthUrl = `https://example.com/auth/${'a'.repeat(4000)}`
    const warnings: string[] = []
    const message = formatAuthUrlMessage(longAuthUrl, msg => warnings.push(msg))
    expect(message).toBe(`Authenticate your account at:\n${longAuthUrl}`)
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toMatch(/^Could not generate a QR code: /)
  })
})
