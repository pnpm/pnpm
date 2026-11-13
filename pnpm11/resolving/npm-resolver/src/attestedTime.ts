import { promises as fs } from 'node:fs'
import path from 'node:path'

import { ATTESTED_TIME_DIR } from '@pnpm/constants'
import { isError } from '@pnpm/error'

import { getPkgMirrorPath } from './metaMirror.js'

export interface AttestedTimeLocation {
  cacheDir: string
  registry: string
  name: string
}

/**
 * The publish times read from the attestation endpoint and kept on disk,
 * keyed by version. An attestation answers for one version, so no packument
 * mirror records it; without this record an offline verification could not
 * repeat a check the attestation endpoint settled online.
 *
 * One file per (registry, name), one JSON line per version. A line is
 * appended whole, so concurrent installs never rewrite each other's lines,
 * and a line that does not parse is skipped. The layout is shared with the
 * Rust stack (`pnpm/crates/resolving-npm-resolver/src/attested_time.rs`).
 */
export async function loadAttestedTimes (location: AttestedTimeLocation): Promise<Record<string, string>> {
  let contents: string
  try {
    contents = await fs.readFile(attestedTimePath(location), 'utf8')
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
  const times: Record<string, string> = {}
  for (const line of contents.split('\n')) {
    const record = parseAttestedTime(line)
    if (record != null) times[record.version] = record.publishedAt
  }
  return times
}

export async function appendAttestedTime (
  location: AttestedTimeLocation,
  record: { version: string, publishedAt: string }
): Promise<void> {
  const file = attestedTimePath(location)
  await fs.mkdir(path.dirname(file), { recursive: true })
  await fs.appendFile(file, `${JSON.stringify({ version: record.version, publishedAt: record.publishedAt })}\n`)
}

function attestedTimePath ({ cacheDir, registry, name }: AttestedTimeLocation): string {
  return getPkgMirrorPath(cacheDir, ATTESTED_TIME_DIR, registry, name)
}

function parseAttestedTime (line: string): { version: string, publishedAt: string } | undefined {
  let record: unknown
  try {
    record = JSON.parse(line)
  } catch {
    return undefined
  }
  if (record == null || typeof record !== 'object') return undefined
  const { version, publishedAt } = record as { version?: unknown, publishedAt?: unknown }
  if (typeof version !== 'string' || typeof publishedAt !== 'string') return undefined
  return { version, publishedAt }
}
