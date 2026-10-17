import type { MetaHeaders, MirrorHeadersBody } from './mirrorLayout.js'

/**
 * The document a metadata request asks for, as pnpm v12's
 * `metadata_cache_policy::DocumentKind`:
 * - `abbreviated` and `full` are the two registry forms, each in its own mirror.
 * - `fullInAbbreviatedMirror` is a full document a release-age upgrade stored
 *   in the abbreviated mirror. Its ETag, recorded as `fullEtag`, validates
 *   only the full representation.
 */
export type DocumentKind = 'abbreviated' | 'full' | 'fullInAbbreviatedMirror'

/** The request shape and validators of one metadata request. */
export interface MetadataRequest {
  fullMetadata: boolean
  etag?: string
  modified?: string
  /** Sends `Cache-Control: no-cache`. */
  cacheBypass: boolean
}

/** The revalidation of a mirror whose header line is `headers`. */
export function conditionalRequest (mirrorFullMetadata: boolean, headers: MetaHeaders | null): MetadataRequest {
  const kind = documentKindOf(mirrorFullMetadata, headers)
  return {
    fullMetadata: kind !== 'abbreviated',
    etag: kind === 'fullInAbbreviatedMirror' ? headers?.fullEtag : headers?.etag,
    modified: headers?.modified,
    cacheBypass: headers?.uncacheable === true,
  }
}

/** The kind a mirror's next revalidation asks for. */
export function documentKindOf (mirrorFullMetadata: boolean, headers: MetaHeaders | null): DocumentKind {
  if (mirrorFullMetadata) return 'full'
  return headers?.fullEtag != null && headers.fullEtag !== '' ? 'fullInAbbreviatedMirror' : 'abbreviated'
}

/**
 * The request after a 304 the mirror cannot serve. A registry can answer it
 * only with a body or an error, never another 304.
 */
export function unconditionalRefetch (fullMetadata: boolean): MetadataRequest {
  return { fullMetadata, cacheBypass: true }
}

/**
 * The release-age upgrade of an abbreviated mirror whose header line is
 * `headers`. A mirror that already holds the full document revalidates it
 * with its `fullEtag`, so a registry with per-representation ETags answers
 * 304 instead of sending the same document again.
 *
 * An abbreviated document sends no validators. An ETag and a `Last-Modified`
 * date describe one representation, and a registry that reuses them across
 * both forms would answer 304, leaving the maturity check without per-version
 * publish dates.
 */
export function releaseAgeUpgradeRequest (headers: MetaHeaders | null): MetadataRequest {
  if (documentKindOf(false, headers) !== 'fullInAbbreviatedMirror') {
    return { fullMetadata: true, cacheBypass: false }
  }
  return { ...conditionalRequest(false, headers), fullMetadata: true }
}

/**
 * The header line a fetched document is mirrored with, the one writer of a
 * fetched document's headers. A full document stored in the abbreviated
 * mirror keeps its ETag as `fullEtag`.
 */
export interface MirrorWrite {
  kind: DocumentKind
  etag: string | undefined
  body: MirrorHeadersBody
}

export function mirrorWrite (
  response: { etag?: string, fullMetadata?: boolean, uncacheable?: boolean },
  mirrorFullMetadata: boolean
): MirrorWrite {
  const kind: DocumentKind = mirrorFullMetadata
    ? 'full'
    : response.fullMetadata === true ? 'fullInAbbreviatedMirror' : 'abbreviated'
  const fullInAbbreviatedMirror = kind === 'fullInAbbreviatedMirror'
  return {
    kind,
    etag: fullInAbbreviatedMirror ? undefined : response.etag,
    body: {
      uncacheable: response.uncacheable,
      fullEtag: fullInAbbreviatedMirror ? response.etag : undefined,
    },
  }
}

/**
 * Whether a mirror the registry marked uncacheable may still answer a pick.
 * Offline and prefer-offline may serve it. Every other online path has to
 * refetch it. The flag is only set on packuments read from the mirror: a
 * document fetched during this install stays reusable for the rest of it.
 */
export function mirrorMayAnswer (
  ctx: { offline?: boolean, preferOffline?: boolean },
  uncacheable: boolean | undefined
): boolean {
  return ctx.offline === true || ctx.preferOffline === true || uncacheable !== true
}
