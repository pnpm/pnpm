import { createHash } from 'node:crypto'
import path from 'node:path'

import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'

/**
 * Per-package summary describing a successful publish, modeled after `npm publish --json`.
 * Returned to callers and serialized to stdout when `pnpm publish --json` is used.
 */
export interface PublishSummary {
  /** Human-readable identifier `${name}@${version}`. */
  id: string
  name: string
  version: string
  /** Compressed tarball size in bytes. */
  size: number
  /** Total uncompressed size of all files in the tarball, in bytes. */
  unpackedSize: number
  /** Lowercase hex SHA-1 digest of the tarball. */
  shasum: string
  /** SRI-formatted SHA-512 digest of the tarball (e.g. `sha512-...`). */
  integrity: string
  /** Tarball file basename (e.g. `pkg-1.0.0.tgz`). */
  filename: string
  /** Files inside the tarball, in the same shape `pnpm pack --json` emits. */
  files: Array<{ path: string }>
  /** Number of files inside the tarball. */
  entryCount: number
  /** Names of bundled dependencies included in the tarball (typically empty). */
  bundled: string[]
  /** Staged publish identifier returned by the registry. Only present for staged publishes. */
  stageId?: string
}

export interface PackedPkgInfo {
  publishedManifest: ExportedManifest
  tarballPath: string
  contents: string[]
  unpackedSize: number
}

export function createPublishSummary (
  { publishedManifest, tarballPath, contents, unpackedSize }: PackedPkgInfo,
  tarballData: Buffer
): PublishSummary {
  const { name, version } = publishedManifest
  return {
    id: `${name}@${version}`,
    name: name as string,
    version: version as string,
    size: tarballData.byteLength,
    unpackedSize,
    // SHA-1 is what `npm publish --json` reports as `shasum` for back-compat with the registry's
    // legacy dist.shasum field; `integrity` below is the modern SRI hash.
    shasum: createHash('sha1').update(tarballData).digest('hex'),
    integrity: `sha512-${createHash('sha512').update(tarballData).digest('base64')}`,
    filename: path.basename(tarballPath),
    files: contents.map((file) => ({ path: file })),
    entryCount: contents.length,
    bundled: extractBundledDependencies(publishedManifest),
  }
}

/**
 * Normalize the two equivalent manifest keys (`bundledDependencies` and `bundleDependencies`)
 * into a flat list of dependency names, matching npm's interpretation.
 */
export function extractBundledDependencies (manifest: {
  bundledDependencies?: unknown
  bundleDependencies?: unknown
  dependencies?: Record<string, unknown>
}): string[] {
  const raw = manifest.bundledDependencies ?? manifest.bundleDependencies
  if (!raw) return []
  if (Array.isArray(raw)) return raw.filter((name): name is string => typeof name === 'string')
  // `true` means "bundle every dependency" per npm's semantics; expand it to the dependency names.
  if (raw === true) return Object.keys(manifest.dependencies ?? {})
  return []
}
