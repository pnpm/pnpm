import { describe, expect, it } from '@jest/globals'
import { integrityToHashes } from '@pnpm/sbom'

describe('integrityToHashes', () => {
  it('should return empty array for undefined', () => {
    expect(integrityToHashes(undefined)).toEqual([])
  })

  it('should return empty array for empty string', () => {
    expect(integrityToHashes('')).toEqual([])
  })

  it('should convert sha512 SRI to hex digest', () => {
    // "hello" in sha512 base64
    const base64Digest = 'm3HZHUS1gluA4SYbwmv9oKmjWpnOkVsNMODkieIEoW4yBFgVi/2ypgiJMOxRaA0MYkJMuxfb+Z1yDFkJUGFnOQ=='
    const integrity = `sha512-${base64Digest}`
    const result = integrityToHashes(integrity)

    expect(result).toHaveLength(1)
    expect(result[0].algorithm).toBe('SHA-512')
    expect(result[0].digest).toMatch(/^[0-9a-f]+$/)
  })

  it('should convert sha256 SRI to hex digest', () => {
    // Some arbitrary sha256 integrity
    const base64Digest = 'LCt5klFGBqVfMfB1GL1o2Ll+0w/DeN2OZGR8U2/9fns='
    const integrity = `sha256-${base64Digest}`
    const result = integrityToHashes(integrity)

    expect(result).toHaveLength(1)
    expect(result[0].algorithm).toBe('SHA-256')
    expect(result[0].digest).toMatch(/^[0-9a-f]+$/)
  })

  it('should handle multiple hash algorithms', () => {
    const sha256 = 'LCt5klFGBqVfMfB1GL1o2Ll+0w/DeN2OZGR8U2/9fns='
    const sha512 = 'm3HZHUS1gluA4SYbwmv9oKmjWpnOkVsNMODkieIEoW4yBFgVi/2ypgiJMOxRaA0MYkJMuxfb+Z1yDFkJUGFnOQ=='
    const integrity = `sha256-${sha256} sha512-${sha512}`
    const result = integrityToHashes(integrity)

    expect(result).toHaveLength(2)
    expect(result.map(h => h.algorithm)).toContain('SHA-256')
    expect(result.map(h => h.algorithm)).toContain('SHA-512')
  })
})
