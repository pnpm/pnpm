import { PnpmError } from '@pnpm/error'

// Matches the integrity format "algo-base64hash"
const INTEGRITY_REGEX = /^([^-]+)-([a-z0-9+/=]+)$/i

export interface ParsedIntegrity {
  algorithm: string
  hexDigest: string
}

/**
 * Parses an integrity string (e.g., "sha512-base64hash") into its components.
 * @throws PnpmError if the integrity format is invalid
 */
export function parseIntegrity (integrity: string): ParsedIntegrity {
  const match = integrity.match(INTEGRITY_REGEX)
  if (!match) {
    throw new PnpmError('INVALID_INTEGRITY', `Invalid integrity format: expected "algo-base64hash", got "${integrity}"`)
  }
  const hexDigest = Buffer.from(match[2], 'base64').toString('hex')
  if (hexDigest.length === 0) {
    throw new PnpmError('INVALID_INTEGRITY', 'Invalid integrity: base64 hash decoded to empty digest')
  }
  return { algorithm: match[1], hexDigest }
}

/**
 * Formats a hex digest into an integrity string (e.g., "sha512-base64hash").
 */
export function formatIntegrity (algorithm: string, hexDigest: string): string {
  return `${algorithm}-${Buffer.from(hexDigest, 'hex').toString('base64')}`
}
