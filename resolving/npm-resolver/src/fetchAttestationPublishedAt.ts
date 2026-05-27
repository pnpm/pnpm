import * as retry from '@zkochan/retry'

import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'

/**
 * Per-version publish timestamp from npm's attestation endpoint —
 * `/-/npm/v1/attestations/<name>@<version>`.
 *
 * The response is a small JSON document containing one or more Sigstore
 * bundles. We read `bundle.verificationMaterial.tlogEntries[].integratedTime`
 * (the Rekor inclusion time) and surface it as an ISO date. This is a
 * couple of seconds after the actual publish — close enough for a
 * release-age policy that operates in minutes/hours/days.
 *
 * We deliberately do **not** verify the Sigstore signature here: the
 * trust model is identical to reading the registry's `time` field on
 * the full metadata document. The win is bandwidth — the attestation
 * payload is tens of KB versus the multi-MB full metadata document, so
 * cold-cache + `--frozen-lockfile` installs against a fleet of
 * provenance-published packages pay far less to verify timestamps.
 *
 * Returns `undefined` when:
 *
 * - The package has no published attestations (`404`).
 * - The response is malformed or missing the timestamp.
 * - The request itself fails (network error, registry 5xx).
 *
 * In all of those cases the caller falls back to fetching full metadata.
 */
export interface FetchAttestationOptions {
  registry: string
  authHeaderValue?: string
}

export async function fetchAttestationPublishedAt (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  version: string,
  opts: FetchAttestationOptions
): Promise<string | undefined> {
  const url = `${opts.registry.replace(/\/$/, '')}/-/npm/v1/attestations/${pkgName}@${version}`
  const retryOperation = retry.operation(fetchOpts.retry)
  return new Promise<string | undefined>((resolve) => {
    retryOperation.attempt(async () => {
      let response: Response
      try {
        response = await fetchOpts.fetch(url, {
          authHeaderValue: opts.authHeaderValue,
          retry: fetchOpts.retry,
          timeout: fetchOpts.timeout,
        })
      } catch {
        // Network errors fall through to the full-metadata path; the
        // caller's `fetchFullMetadataCached` has its own retry policy.
        resolve(undefined)
        return
      }
      // 404 = package never published attestations. Other 4xx/5xx also
      // mean "can't get an answer from this endpoint, fall back."
      if (response.status >= 400) {
        resolve(undefined)
        return
      }
      let body: unknown
      try {
        body = await response.json()
      } catch {
        resolve(undefined)
        return
      }
      resolve(extractPublishedAt(body))
    })
  })
}

/**
 * Pull the earliest `integratedTime` across every attestation bundle in
 * the response and convert it to an ISO timestamp. Earliest is the
 * conservative choice: if two attestations disagree (e.g. publish
 * v0.1 vs SLSA provenance v1), we attribute the publish to the older
 * Rekor entry. The Rekor timestamp is what tells us when the artifact
 * existed in a transparency log — that's the floor on publish time.
 */
function extractPublishedAt (body: unknown): string | undefined {
  if (!body || typeof body !== 'object') return undefined
  const attestations = (body as { attestations?: unknown }).attestations
  if (!Array.isArray(attestations)) return undefined

  let earliestSeconds: number | undefined
  for (const attestation of attestations) {
    const seconds = readEarliestIntegratedTime(attestation)
    if (seconds == null) continue
    if (earliestSeconds == null || seconds < earliestSeconds) {
      earliestSeconds = seconds
    }
  }
  if (earliestSeconds == null) return undefined
  return new Date(earliestSeconds * 1000).toISOString()
}

function readEarliestIntegratedTime (attestation: unknown): number | undefined {
  if (!attestation || typeof attestation !== 'object') return undefined
  const bundle = (attestation as { bundle?: unknown }).bundle
  if (!bundle || typeof bundle !== 'object') return undefined
  const verificationMaterial = (bundle as { verificationMaterial?: unknown }).verificationMaterial
  if (!verificationMaterial || typeof verificationMaterial !== 'object') return undefined
  const tlogEntries = (verificationMaterial as { tlogEntries?: unknown }).tlogEntries
  if (!Array.isArray(tlogEntries)) return undefined

  let earliest: number | undefined
  for (const entry of tlogEntries) {
    if (!entry || typeof entry !== 'object') continue
    const rawIntegratedTime = (entry as { integratedTime?: unknown }).integratedTime
    // npm serializes integratedTime as a string ("1778583836") to avoid
    // JSON precision loss; accept either string or number defensively.
    const seconds = parseIntegratedTimeSeconds(rawIntegratedTime)
    if (seconds == null) continue
    if (earliest == null || seconds < earliest) earliest = seconds
  }
  return earliest
}

function parseIntegratedTimeSeconds (raw: unknown): number | undefined {
  const seconds = typeof raw === 'string' ? Number(raw) : typeof raw === 'number' ? raw : NaN
  if (!Number.isFinite(seconds) || seconds <= 0) return undefined
  return seconds
}
