import { createHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
} from '@pnpm/fetching-types'

export async function fetchShasumsFile (
  fetch: FetchFromRegistry,
  shasumsUrl: string,
  expectedVersionIntegrity?: string
): Promise<string> {
  const res = await fetch(shasumsUrl)
  if (!res.ok) {
    throw new PnpmError(
      'NODE_FETCH_INTEGRITY_FAILED',
      `Failed to fetch integrity file: ${shasumsUrl} (status: ${res.status})`
    )
  }

  const body = await res.text()
  if (expectedVersionIntegrity) {
    const actualVersionIntegrity = createHash(body)
    if (expectedVersionIntegrity !== actualVersionIntegrity) {
      throw new PnpmError('NODE_VERSION_INTEGRITY_MISMATCH', `The integrity of ${shasumsUrl} failed. Expected: ${expectedVersionIntegrity}. Actual: ${actualVersionIntegrity}`)
    }
  }
  return body
}

const SHA256_REGEX = /^[a-f0-9]{64}$/

export function pickFileChecksumFromShasumsFile (body: string, fileName: string): string {
  const line = body.split('\n').find(line => line.trim().endsWith(`  ${fileName}`))

  if (!line) {
    throw new PnpmError(
      'NODE_INTEGRITY_HASH_NOT_FOUND',
      `SHA-256 hash not found in SHASUMS256.txt for: ${fileName}`
    )
  }

  const [sha256] = line.trim().split(/\s+/)
  if (!SHA256_REGEX.test(sha256)) {
    throw new PnpmError(
      'NODE_MALFORMED_INTEGRITY_HASH',
      `Malformed SHA-256 for ${fileName}: ${sha256}`
    )
  }

  const buffer = Buffer.from(sha256, 'hex')
  const base64 = buffer.toString('base64')
  return `sha256-${base64}`
}
