import { createHash } from 'node:crypto'

/**
 * Hex digest identifying an artifact blob's content, from its `sha512-` value.
 *
 * The same identity the store addresses its content by, so a caller holding a
 * manifest entry can ask the store whether it already has the bytes. Callers
 * that only need the integrity checked can discard the result.
 *
 * @throws if `integrity` is not a `sha512-` value carrying a 64-byte digest.
 */
export function artifactBlobDigest (integrity: string): string {
  if (typeof integrity !== 'string' || !integrity.startsWith('sha512-')) {
    throw new Error('Shared artifact blobs require sha512 integrity')
  }
  const digest = decodeBase64('sha512 digest', integrity.slice('sha512-'.length))
  if (digest.byteLength !== 64) throw new Error(`SHA-512 digest is ${digest.byteLength} bytes instead of 64`)
  return digest.toString('hex')
}

export function verifyBlob (integrity: string, bytes: Buffer): void {
  const expected = artifactBlobDigest(integrity)
  const actual = createHash('sha512').update(bytes).digest('hex')
  if (expected !== actual) throw new Error('Downloaded shared artifact blob does not match its declared digest')
}

export function validateScalar (label: string, value: unknown, maxLength: number): asserts value is string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    Buffer.byteLength(value) > maxLength ||
    Array.from(value).some(character => isControl(character))
  ) {
    throw new Error(`Shared artifact ${label} is empty, too long, or contains a control character`)
  }
}

export function isControl (character: string): boolean {
  const codePoint = character.codePointAt(0)!
  return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f)
}

export function decodeBase64 (label: string, encoded: string, allowEmpty = false): Buffer {
  if (typeof encoded !== 'string' || (!allowEmpty && encoded.length === 0) || /\s/.test(encoded)) {
    throw new Error(`Shared artifact ${label} is not valid base64`)
  }
  const decoded = Buffer.from(encoded, 'base64')
  if (decoded.toString('base64') !== encoded) {
    throw new Error(`Shared artifact ${label} is not valid base64`)
  }
  return decoded
}
