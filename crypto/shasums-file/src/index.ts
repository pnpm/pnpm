import { PnpmError } from '@pnpm/error'
import type {
  FetchFromRegistry,
} from '@pnpm/fetching.types'

import { fetchVerifiedNodeShasums } from './verifyNodeShasums.js'

export { fetchVerifiedNodeShasums }

export interface ShasumsFileItem {
  integrity: string
  fileName: string
}

export async function fetchShasumsFile (
  fetch: FetchFromRegistry,
  shasumsUrl: string
): Promise<ShasumsFileItem[]> {
  return parseShasumsFile(await fetchShasumsFileRaw(fetch, shasumsUrl))
}

/**
 * Like {@link fetchShasumsFile}, but first verifies the SHASUMS file's detached
 * OpenPGP signature against the Node.js release keys (see
 * {@link fetchVerifiedNodeShasums}). Use this whenever the SHASUMS file is
 * fetched from a repository-configurable Node.js mirror.
 */
export async function fetchVerifiedNodeShasumsFile (
  fetch: FetchFromRegistry,
  shasumsUrl: string
): Promise<ShasumsFileItem[]> {
  return parseShasumsFile(await fetchVerifiedNodeShasums(fetch, shasumsUrl))
}

export function parseShasumsFile (shasumsFileContent: string): ShasumsFileItem[] {
  const lines = shasumsFileContent.split('\n')
  const items: ShasumsFileItem[] = []
  for (const line of lines) {
    if (!line) continue
    const [sha256, fileName] = line.trim().split(/\s+/)
    items.push({
      integrity: `sha256-${Buffer.from(sha256, 'hex').toString('base64')}`,
      fileName,
    })
  }
  return items
}

export async function fetchShasumsFileRaw (
  fetch: FetchFromRegistry,
  shasumsUrl: string
): Promise<string> {
  const res = await fetch(shasumsUrl)
  if (!res.ok) {
    throw new PnpmError(
      'FAILED_DOWNLOAD_SHASUM_FILE',
      `Failed to fetch integrity file: ${shasumsUrl} (status: ${res.status})`
    )
  }
  const body = await res.text()
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
