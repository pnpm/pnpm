import { describe, expect, it } from '@jest/globals'
import { formatAuthUrlMessage, generateQrCode } from '@pnpm/network.web-auth'

describe('formatAuthUrlMessage', () => {
  it('appends a QR code when one can be generated', () => {
    const authUrl = 'https://example.com/auth'
    const message = formatAuthUrlMessage(authUrl, msg => {
      throw new Error(`Unexpected call to globalWarn: ${msg}`)
    })
    expect(message).toBe(`Authenticate your account at:\n${authUrl}\n\n${generateQrCode(authUrl)}`)
  })

  it('warns and falls back to a URL-only message when QR generation fails', () => {
    // Longer than the 2953-byte maximum QR data capacity (version 40 at
    // error-correction level L), which makes qrcode-terminal throw.
    const longAuthUrl = `https://example.com/auth/${'a'.repeat(4000)}`
    const warnings: string[] = []
    const message = formatAuthUrlMessage(longAuthUrl, msg => warnings.push(msg))
    expect(message).toBe(`Authenticate your account at:\n${longAuthUrl}`)
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toContain('Could not generate a QR code:')
  })
})
