import { PnpmError } from '@pnpm/error'
import type {
  FetchFromRegistry,
} from '@pnpm/fetching.types'

import { readCachedBytes, readCachedShasums, RUNTIME_SHASUMS_DIR, writeCachedShasums } from './diskCache.js'
import {
  type ArmoredKey,
  fetchVerifiedNodeShasums,
  fetchVerifiedNodeShasumsWithSignature,
  nodeShasumsSignatureVerifies,
} from './verifyNodeShasums.js'

export { fetchVerifiedNodeShasums, RUNTIME_SHASUMS_DIR }

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

export interface FetchShasumsFileCachedOpts {
  cacheDir?: string
}

export interface FetchVerifiedNodeShasumsFileCachedOpts extends FetchShasumsFileCachedOpts {
  trustedKeys?: readonly ArmoredKey[]
}

/**
 * Like {@link fetchVerifiedNodeShasumsFile}, backed by the disk cache when
 * `opts.cacheDir` is given. The cache stores the body together with its
 * detached signature, and a cache hit re-verifies that signature against the
 * embedded release keys: the cache directory is project-configurable, so a
 * pre-seeded entry must prove it is a genuine release body before it is
 * served. Any verification failure is a miss and the pair is refetched.
 * `shasumsUrl` must be version-pinned — a mutable URL must never be handed to
 * the cache.
 */
export async function fetchVerifiedNodeShasumsFileCached (
  fetch: FetchFromRegistry,
  shasumsUrl: string,
  opts?: FetchVerifiedNodeShasumsFileCachedOpts
): Promise<ShasumsFileItem[]> {
  const cacheOpts = { cacheDir: opts?.cacheDir, trust: 'verified' as const }
  const signatureUrl = `${shasumsUrl}.sig`
  const [cachedBody, cachedSignature] = await Promise.all([
    readCachedShasums(shasumsUrl, cacheOpts),
    readCachedBytes(signatureUrl, cacheOpts),
  ])
  if (
    cachedBody != null && cachedSignature != null &&
    await nodeShasumsSignatureVerifies(Buffer.from(cachedBody, 'utf8'), cachedSignature, opts?.trustedKeys)
  ) {
    return parseShasumsFile(cachedBody)
  }
  const { body, signature } = await fetchVerifiedNodeShasumsWithSignature(fetch, shasumsUrl, opts?.trustedKeys)
  await Promise.all([
    writeCachedShasums(shasumsUrl, body, cacheOpts),
    writeCachedShasums(signatureUrl, signature, cacheOpts),
  ])
  return parseShasumsFile(body)
}

/**
 * Like {@link fetchShasumsFile}, backed by the disk cache when `opts.cacheDir`
 * is given. For mirrors whose SHASUMS files carry no verifiable signature the
 * cached body is trusted exactly as far as the TLS fetch that produced it.
 * `shasumsUrl` must be version-pinned — a mutable URL must never be handed to
 * the cache.
 */
export async function fetchShasumsFileCached (
  fetch: FetchFromRegistry,
  shasumsUrl: string,
  opts?: FetchShasumsFileCachedOpts
): Promise<ShasumsFileItem[]> {
  const cacheOpts = { cacheDir: opts?.cacheDir, trust: 'unverified' as const }
  const cached = await readCachedShasums(shasumsUrl, cacheOpts)
  if (cached != null) return parseShasumsFile(cached)
  const body = await fetchShasumsFileRaw(fetch, shasumsUrl)
  await writeCachedShasums(shasumsUrl, body, cacheOpts)
  return parseShasumsFile(body)
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
