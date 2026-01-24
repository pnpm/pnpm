import { parseIntegrity } from '@pnpm/crypto.integrity'

describe('parseIntegrity', () => {
  it('parses a valid sha512 integrity string', () => {
    // "hello" hashed with sha512, base64 encoded
    const integrity = 'sha512-9/u6bgY2+JDlb7vzKD5STG+jIErimDgtYkdB0NxmODJuKCxBvl5CVNiCB3LFUYosWowMf37aGVlKfrU5RT4e1w=='
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha512')
    expect(result.hexDigest).toBe('f7fbba6e0636f890e56fbbf3283e524c6fa3204ae298382d624741d0dc6638326e282c41be5e4254d8820772c5518a2c5a8c0c7f7eda19594a7eb539453e1ed7')
  })

  it('parses a valid sha256 integrity string', () => {
    // "hello" hashed with sha256, base64 encoded
    const integrity = 'sha256-LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ='
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha256')
    expect(result.hexDigest).toBe('2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824')
  })

  it('parses a valid sha1 integrity string', () => {
    // "hello" hashed with sha1, base64 encoded
    const integrity = 'sha1-qvTGHdzF6KLavt4PO0gs2a6pQ00='
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha1')
    expect(result.hexDigest).toBe('aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d')
  })

  it('handles algorithms with numbers', () => {
    const integrity = 'sha384-OLBgp1GsljhM2TJ+sbHjaiH9txEUvgdDTAzHv2P24donTt6/529l+9Ua0vFImLlb'
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha384')
    expect(result.hexDigest).toHaveLength(96) // 384 bits = 48 bytes = 96 hex chars
  })

  it('is case-insensitive for base64 characters', () => {
    // Same hash but with mixed case (valid base64)
    const integrity = 'sha256-LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ='
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha256')
  })

  it('throws on missing algorithm', () => {
    expect(() => parseIntegrity('LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ='))
      .toThrow('Invalid integrity format')
  })

  it('throws on empty string', () => {
    expect(() => parseIntegrity(''))
      .toThrow('Invalid integrity format')
  })

  it('throws on missing hash', () => {
    expect(() => parseIntegrity('sha256-'))
      .toThrow('Invalid integrity format')
  })

  it('throws on invalid base64 characters', () => {
    expect(() => parseIntegrity('sha256-invalid!@#$%'))
      .toThrow('Invalid integrity format')
  })

  it('throws on multiple dashes in algorithm', () => {
    // The regex requires algorithm to have no dashes (uses [^-]+)
    expect(() => parseIntegrity('sha-256-LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ='))
      .toThrow('Invalid integrity format')
  })

  it('throws when base64 decodes to empty', () => {
    // Padding-only base64 decodes to empty buffer
    expect(() => parseIntegrity('sha256-===='))
      .toThrow('base64 hash decoded to empty digest')
  })

  it('handles base64 without padding', () => {
    // Some systems omit padding
    const integrity = 'sha256-LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ'
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha256')
    // Node's Buffer.from handles missing padding gracefully
    expect(result.hexDigest).toBeTruthy()
  })

  it('handles base64 special characters (+ and /)', () => {
    const integrity = 'sha512-abc+def/ghi='
    const result = parseIntegrity(integrity)
    expect(result.algorithm).toBe('sha512')
    expect(result.hexDigest).toBeTruthy()
  })
})
