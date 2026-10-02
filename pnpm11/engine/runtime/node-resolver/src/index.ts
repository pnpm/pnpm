import { fetchShasumsFileCached, FetchShasumsFileError, fetchVerifiedNodeShasumsFileCached } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching.types'
import type {
  BinaryResolution,
  LatestInfo,
  LatestQuery,
  PlatformAssetResolution,
  PlatformAssetTarget,
  ResolveOptions,
  ResolveResult,
  VariationsResolution,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'
import type { PkgResolutionId } from '@pnpm/types'
import semver from 'semver'

import { getNodeArtifactAddress } from './getNodeArtifactAddress.js'
import { getNodeMirror } from './getNodeMirror.js'
import { parseNodeSpecifier } from './parseNodeSpecifier.js'
import {
  createAuthenticatedFetch,
  getSecureAuthHeader,
  type NodeVersion,
  type NodeVersionFetchOptions,
  normalizeNodeVersionFetchOptions,
  normalizeRuntimeSpec,
  resolveNodeVersion,
  resolveNodeVersions,
} from './resolveNodeVersion.js'

export {
  getNodeArtifactAddress,
  getNodeMirror,
  type NodeVersion,
  type NodeVersionFetchOptions,
  normalizeNodeVersionFetchOptions,
  normalizeRuntimeSpec,
  parseNodeSpecifier,
  resolveNodeVersion,
  resolveNodeVersions,
}

export const DEFAULT_NODE_MIRROR_BASE_URL = 'https://nodejs.org/download/release/'
export const UNOFFICIAL_NODE_MIRROR_BASE_URL = 'https://unofficial-builds.nodejs.org/download/release/'

// Node.js archives ship with npm, npx, and corepack. pnpm manages package managers itself,
// so these are excluded from the runtime install — skipping ~2,800 files out of ~5,800 in the
// Node.js tarball. The pattern matches paths *after* the archive's top-level
// `node-vX.Y.Z-<platform>-<arch>/` prefix has been stripped.
export const NODE_EXTRAS_IGNORE_PATTERN = '^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\\.(?:cmd|ps1))?$)'

export interface NodeRuntimeResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'nodejs.org'
}

export async function resolveNodeRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    getAuthHeader?: GetAuthHeader
    nodeDownloadMirrors?: Record<string, string>
    offline?: boolean
    cacheDir?: string
  },
  wantedDependency: WantedDependency,
  opts?: Partial<ResolveOptions>
): Promise<NodeRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'node' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null

  if (opts?.currentPkg && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as VariationsResolution,
      resolvedVia: 'nodejs.org',
    }
  }

  if (ctx.offline) throw new PnpmError('NO_OFFLINE_NODEJS_RESOLUTION', 'Offline Node.js resolution is not supported')
  const versionSpec = normalizeRuntimeSpec(wantedDependency.bareSpecifier.substring('runtime:'.length))
  const fetch = createAuthenticatedFetch(ctx.fetchFromRegistry, ctx.getAuthHeader)
  const { version, variants } = await resolveVersionAndVariants(
    fetch,
    ctx,
    versionSpec
  )
  const range = createNodeRuntimeVersionSpec(versionSpec, version, wantedDependency)
  return {
    id: `node@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${range}`,
    resolvedVia: 'nodejs.org',
    manifest: {
      name: 'node',
      version,
      bin: getNodeBinsForCurrentOS(),
    },
    resolution: {
      type: 'variations',
      variants,
    },
  }
}

export async function resolveLatestNodeRuntime (
  ctx: { fetchFromRegistry: FetchFromRegistry, getAuthHeader?: GetAuthHeader, nodeDownloadMirrors?: Record<string, string> },
  query: LatestQuery,
  _opts: ResolveOptions
): Promise<LatestInfo | undefined> {
  const manifestSpec = query.wantedDependency.bareSpecifier
  if (query.wantedDependency.alias !== 'node' || !manifestSpec?.startsWith('runtime:')) return undefined
  const versionSpec = query.compatible ? normalizeRuntimeSpec(manifestSpec.substring('runtime:'.length)) : 'latest'
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.nodeDownloadMirrors, releaseChannel)
  const version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpecifier, {
    nodeMirrorBaseUrl,
    getAuthHeader: ctx.getAuthHeader,
  })
  if (!version) return {}
  return { latestManifest: { name: 'node', version } }
}

function createNodeRuntimeVersionSpec (
  versionSpec: string,
  resolvedVersion: string,
  wantedDependency: WantedDependency
): string {
  if (resolvedVersion === versionSpec || semver.parse(resolvedVersion)?.prerelease.length) {
    return resolvedVersion
  }
  const source = wantedDependency.prevSpecifier?.startsWith('runtime:')
    ? wantedDependency.prevSpecifier.substring('runtime:'.length)
    : versionSpec
  const spec = source.includes('/') ? source.split('/', 2)[1] : source
  if (spec.startsWith('^')) return `^${resolvedVersion}`
  if (spec.startsWith('~')) return `~${resolvedVersion}`
  return resolvedVersion
}

/**
 * The concrete version an exact stable-release specifier names, when the
 * specifier is already in canonical `X.Y.Z` form. Such a specifier needs no
 * release-index lookup: the index would resolve it to itself. Prereleases are
 * excluded — on the `release` channel they never exist, so routing them
 * through the index keeps the canonical not-found error path.
 */
function exactReleaseVersion (releaseChannel: string, versionSpecifier: string): string | undefined {
  if (releaseChannel !== 'release') return undefined
  const parsed = semver.parse(versionSpecifier)
  if (parsed == null || parsed.prerelease.length > 0 || parsed.build.length > 0 || parsed.version !== versionSpecifier) return undefined
  return parsed.version
}

async function versionMissingFromIndex (fetch: FetchFromRegistry, version: string, nodeMirrorBaseUrl: string): Promise<boolean> {
  try {
    return (await resolveNodeVersion(fetch, version, nodeMirrorBaseUrl)) == null
  } catch {
    return false
  }
}

async function readNodeAssets (
  fetch: FetchFromRegistry,
  opts: {
    nodeMirrorBaseUrl: string
    version: string
    releaseChannel: string
    cacheDir?: string
    getAuthHeader?: GetAuthHeader
  }
): Promise<PlatformAssetResolution[]> {
  const { nodeMirrorBaseUrl, version, releaseChannel, cacheDir, getAuthHeader } = opts
  // The mirror is repository-configurable, so the SHASUMS file's hashes are only
  // trustworthy once its OpenPGP signature is verified against the Node.js
  // release keys embedded in pnpm. Only the `release` channel publishes a signed
  // SHASUMS256.txt; pre-release channels (rc, nightly, ...) are unsigned by Node,
  // so they cannot be verified this way.
  const assets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl, version, muslOnly: false, verifySignature: releaseChannel === 'release', cacheDir, getAuthHeader })

  // When using the default mirror, also fetch musl variants from unofficial-builds.nodejs.org,
  // since musl builds are not available on the official mirror. That URL is hardcoded (not
  // repository-configurable) and signed by a different (unofficial-builds) key, so it is trusted
  // over TLS rather than verified against the official release keys.
  if (nodeMirrorBaseUrl === DEFAULT_NODE_MIRROR_BASE_URL) {
    try {
      const muslAssets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl: UNOFFICIAL_NODE_MIRROR_BASE_URL, version, muslOnly: true, verifySignature: false, cacheDir, getAuthHeader })
      assets.push(...muslAssets)
    } catch (err: unknown) {
      // 404 is how unofficial-builds reports a release it never built, which
      // is the case for very old Node.js versions. Every other failure aborts
      // the resolve: an unreachable or blocked mirror must not silently drop
      // the musl assets, because that writes a lockfile that differs from the
      // one the same command produces elsewhere.
      if (!(err instanceof FetchShasumsFileError) || err.status !== 404) throw err
    }
  }

  return assets
}

async function resolveVersionAndVariants (
  fetch: FetchFromRegistry,
  ctx: {
    nodeDownloadMirrors?: Record<string, string>
    cacheDir?: string
    getAuthHeader?: GetAuthHeader
  },
  versionSpec: string
): Promise<{ version: string, variants: PlatformAssetResolution[] }> {
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.nodeDownloadMirrors, releaseChannel)
  const exactVersion = exactReleaseVersion(releaseChannel, versionSpecifier)
  const version = exactVersion ?? await resolveNodeVersionOrThrow(fetch, versionSpecifier, nodeMirrorBaseUrl, versionSpec)

  try {
    const variants = await readNodeAssets(fetch, {
      nodeMirrorBaseUrl,
      version,
      releaseChannel,
      cacheDir: ctx.cacheDir,
      getAuthHeader: ctx.getAuthHeader,
    })
    return { version, variants }
  } catch (err: unknown) {
    if (exactVersion != null && await versionMissingFromIndex(fetch, exactVersion, nodeMirrorBaseUrl)) {
      throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
    }
    throw err
  }
}

async function resolveNodeVersionOrThrow (
  fetch: FetchFromRegistry,
  versionSpecifier: string,
  nodeMirrorBaseUrl: string,
  versionSpec: string
): Promise<string> {
  const version = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  if (!version) {
    throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
  }
  return version
}

async function readNodeAssetsFromMirror (
  fetch: FetchFromRegistry,
  opts: {
    nodeMirrorBaseUrl: string
    version: string
    muslOnly: boolean
    verifySignature: boolean
    cacheDir?: string
    getAuthHeader?: GetAuthHeader
  }
): Promise<PlatformAssetResolution[]> {
  const { nodeMirrorBaseUrl, version, muslOnly, verifySignature, getAuthHeader } = opts
  const integritiesFileUrl = `${nodeMirrorBaseUrl}v${version}/SHASUMS256.txt`
  const cacheOpts = {
    cacheDir: opts.cacheDir,
    skipCache: getSecureAuthHeader(getAuthHeader, integritiesFileUrl) != null ||
      getSecureAuthHeader(getAuthHeader, `${integritiesFileUrl}.sig`) != null,
  }
  const shasumsFileItems = verifySignature
    ? await fetchVerifiedNodeShasumsFileCached(fetch, integritiesFileUrl, cacheOpts)
    : await fetchShasumsFileCached(fetch, integritiesFileUrl, cacheOpts)
  return filterAndParseNodeAssets(shasumsFileItems, version, nodeMirrorBaseUrl, muslOnly)
}

function filterAndParseNodeAssets (
  items: Array<{ integrity: string, fileName: string }>,
  version: string,
  nodeMirrorBaseUrl: string,
  muslOnly: boolean
): PlatformAssetResolution[] {
  const escaped = version.replace(/\\/g, '\\\\').replace(/\./g, '\\.')
  const pattern = new RegExp(`^node-v${escaped}-([^-.]+)-([^.-]+)(-musl)?\\.(?:tar\\.gz|zip)$`)
  const assets: PlatformAssetResolution[] = []
  for (const item of items) {
    const asset = parseNodeAsset(item, pattern, version, nodeMirrorBaseUrl, muslOnly)
    if (asset != null) assets.push(asset)
  }
  return assets
}

function getNodeAssetTargets (platform: string, arch: string, libc: 'musl' | undefined, version: string): PlatformAssetTarget[] {
  const targets: PlatformAssetTarget[] = [{
    os: platform,
    cpu: arch,
    ...(libc != null && { libc }),
  }]
  const nodeMajorVersion = +version.split('.')[0]
  if (platform === 'darwin' && arch === 'x64' && nodeMajorVersion < 16) {
    targets.push({ os: 'darwin', cpu: 'arm64' })
  }
  if (platform === 'win32' && arch === 'x64' && nodeMajorVersion < 20) {
    targets.push({ os: 'win32', cpu: 'arm64' })
  }
  return targets
}

function parseNodeAsset (
  item: { integrity: string, fileName: string },
  pattern: RegExp,
  version: string,
  nodeMirrorBaseUrl: string,
  muslOnly: boolean
): PlatformAssetResolution | undefined {
  const match = pattern.exec(item.fileName)
  if (!match) return undefined

  const [, rawPlatform, arch, muslSuffix] = match
  const platform = rawPlatform === 'win' ? 'win32' : rawPlatform
  const isMusl = muslSuffix != null
  if (muslOnly && !isMusl) return undefined

  const libc: 'musl' | undefined = isMusl ? 'musl' : undefined
  const address = getNodeArtifactAddress({
    version,
    baseUrl: nodeMirrorBaseUrl,
    platform,
    arch,
    libc,
  })
  const url = `${address.dirname}/${address.basename}${address.extname}`
  const resolution: BinaryResolution = {
    type: 'binary',
    archive: address.extname === '.zip' ? 'zip' : 'tarball',
    bin: getNodeBinsForCurrentOS(platform),
    integrity: item.integrity,
    url,
  }
  if (resolution.archive === 'zip') {
    resolution.prefix = address.basename
  }
  return {
    targets: getNodeAssetTargets(platform, arch, libc, version),
    resolution,
  }
}


function getNodeBinsForCurrentOS (platform: string = process.platform): Record<string, string> {
  if (platform === 'win32') {
    return { node: 'node.exe' }
  }
  return { node: 'bin/node' }
}
