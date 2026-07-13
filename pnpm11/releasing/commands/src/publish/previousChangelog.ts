import { gunzipSync } from 'node:zlib'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import { fetchMetadataFromFromRegistry } from '@pnpm/resolving.npm-resolver'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { lt, rsort, valid } from 'semver'
import tar from 'tar-stream'

export type PreviousChangelogOptions = CreateFetchFromRegistryOptions & Pick<Config,
| 'registries'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
>

const CHANGELOG_ENTRY = 'package/CHANGELOG.md'

/**
 * The `CHANGELOG.md` packed into the highest published version of `pkgName`
 * that is semver-lower than `version` — the changelog the section for
 * `version` is prepended onto in `registry` storage. `undefined` when the
 * package is new, has no earlier published version, or that version's tarball
 * carried no changelog.
 */
export async function fetchPreviousChangelog (opts: PreviousChangelogOptions, pkgName: string, version: string): Promise<string | undefined> {
  const meta = await fetchPackument(opts, pkgName)
  if (meta == null) return undefined
  const previousVersion = pickPreviousVersion(meta, version)
  if (previousVersion == null) return undefined
  return downloadTarballChangelog(opts, pkgName, meta, previousVersion)
}

/**
 * The `CHANGELOG.md` packed into the published `pkgName@version` exactly, or
 * `undefined` when that version is not published (or carried no changelog).
 * Used to confirm a release actually carries its composed section before the
 * intents behind it are garbage-collected.
 */
export async function fetchPublishedChangelog (opts: PreviousChangelogOptions, pkgName: string, version: string): Promise<string | undefined> {
  const meta = await fetchPackument(opts, pkgName)
  if (meta?.versions[version] == null) return undefined
  return downloadTarballChangelog(opts, pkgName, meta, version)
}

export function changelogHasSection (changelog: string, section: string): boolean {
  return changelog.includes(section.trim())
}

async function fetchPackument (opts: PreviousChangelogOptions, pkgName: string): Promise<PackageMeta | undefined> {
  const registry = pickRegistryForPackage(opts.registries, pkgName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  let fetchResult
  try {
    fetchResult = await fetchMetadataFromFromRegistry(
      {
        fetch: fetchFromRegistry,
        retry: {
          factor: opts.fetchRetryFactor,
          maxTimeout: opts.fetchRetryMaxtimeout,
          minTimeout: opts.fetchRetryMintimeout,
          retries: opts.fetchRetries,
        },
        timeout: opts.fetchTimeout ?? 60000,
        fetchWarnTimeoutMs: 10000,
      },
      pkgName,
      {
        registry,
        authHeaderValue: getAuthHeader(registry, { pkgName }),
        fullMetadata: false,
      }
    )
  } catch (err: unknown) {
    // A package with no published version yet has no changelog to build on.
    if (err != null && typeof err === 'object' && 'code' in err && err.code === 'ERR_PNPM_FETCH_404') {
      return undefined
    }
    throw err
  }
  return fetchResult.notModified ? undefined : fetchResult.meta
}

/** Highest published version of the package that is semver-lower than `version`. */
function pickPreviousVersion (meta: PackageMeta, version: string): string | undefined {
  const candidates = Object.keys(meta.versions).filter((candidate) => valid(candidate) != null && lt(candidate, version))
  return candidates.length > 0 ? rsort(candidates)[0] : undefined
}

async function downloadTarballChangelog (opts: PreviousChangelogOptions, pkgName: string, meta: PackageMeta, version: string): Promise<string | undefined> {
  const tarballUrl = meta.versions[version]?.dist?.tarball
  if (tarballUrl == null) return undefined
  const fetchFromRegistry: FetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  const response = await fetchFromRegistry(tarballUrl, { authHeaderValue: getAuthHeader(tarballUrl, { pkgName }) })
  if (!response.ok) {
    throw new PnpmError('CHANGELOG_TARBALL_FETCH_FAILED', `Failed to download ${pkgName}@${version} tarball (${response.status}) to compose the changelog: ${tarballUrl}`)
  }
  return extractTarballEntry(Buffer.from(await response.arrayBuffer()), CHANGELOG_ENTRY)
}

/** Reads one entry's contents out of a (optionally gzipped) tarball buffer. */
async function extractTarballEntry (tarballData: Buffer, entryName: string): Promise<string | undefined> {
  const extract = tar.extract()
  let contents: string | undefined
  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, stream, next) => {
      if (header.name !== entryName) {
        stream.resume()
        stream.on('end', next)
        stream.on('error', reject)
        return
      }
      const chunks: Buffer[] = []
      stream.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)))
      stream.on('error', reject)
      stream.on('end', () => {
        contents = Buffer.concat(chunks).toString('utf8')
        next()
      })
    })
    extract.on('error', reject)
    extract.on('finish', resolve)
    extract.end(maybeGunzip(tarballData))
  })
  return contents
}

function maybeGunzip (tarballData: Buffer): Buffer {
  try {
    return gunzipSync(tarballData)
  } catch {
    return tarballData
  }
}
