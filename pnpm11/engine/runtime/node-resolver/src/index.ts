import { fetchShasumsFileCached, fetchVerifiedNodeShasumsFileCached } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
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
import versionSelectorType from 'version-selector-type'

import { getNodeArtifactAddress } from './getNodeArtifactAddress.js'
import { getNodeMirror } from './getNodeMirror.js'
import { parseNodeSpecifier } from './parseNodeSpecifier.js'

export { getNodeArtifactAddress, getNodeMirror, parseNodeSpecifier }

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
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.nodeDownloadMirrors, releaseChannel)
  // An exact stable-release specifier is its own resolution, so the
  // release-index fetch is skipped for it and existence is proven by the
  // asset-list fetch below.
  const exactVersion = exactReleaseVersion(releaseChannel, versionSpecifier)
  let version = exactVersion
  if (version == null) {
    version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpecifier, nodeMirrorBaseUrl) ?? undefined
    if (!version) {
      throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
    }
  }
  let variants: PlatformAssetResolution[]
  try {
    variants = await readNodeAssets(ctx.fetchFromRegistry, { nodeMirrorBaseUrl, version, releaseChannel, cacheDir: ctx.cacheDir })
  } catch (err: unknown) {
    // The exact-specifier pick skipped the release index, so a failed asset
    // read is ambiguous: the version may simply not exist. Consult the index
    // now, purely to raise the same NODEJS_VERSION_NOT_FOUND the index-first
    // path raises for a nonexistent version; any other outcome re-raises the
    // asset error unchanged.
    if (exactVersion != null && await versionMissingFromIndex(ctx.fetchFromRegistry, exactVersion, nodeMirrorBaseUrl)) {
      throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
    }
    throw err
  }
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
  ctx: { fetchFromRegistry: FetchFromRegistry, nodeDownloadMirrors?: Record<string, string> },
  query: LatestQuery,
  _opts: ResolveOptions
): Promise<LatestInfo | undefined> {
  const manifestSpec = query.wantedDependency.bareSpecifier
  if (query.wantedDependency.alias !== 'node' || !manifestSpec?.startsWith('runtime:')) return undefined
  const versionSpec = query.compatible ? normalizeRuntimeSpec(manifestSpec.substring('runtime:'.length)) : 'latest'
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.nodeDownloadMirrors, releaseChannel)
  const version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpecifier, nodeMirrorBaseUrl)
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
  }
): Promise<PlatformAssetResolution[]> {
  const { nodeMirrorBaseUrl, version, releaseChannel, cacheDir } = opts
  // The mirror is repository-configurable, so the SHASUMS file's hashes are only
  // trustworthy once its OpenPGP signature is verified against the Node.js
  // release keys embedded in pnpm. Only the `release` channel publishes a signed
  // SHASUMS256.txt; pre-release channels (rc, nightly, …) are unsigned by Node,
  // so they cannot be verified this way.
  const assets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl, version, muslOnly: false, verifySignature: releaseChannel === 'release', cacheDir })

  // When using the default mirror, also fetch musl variants from unofficial-builds.nodejs.org,
  // since musl builds are not available on the official mirror. That URL is hardcoded (not
  // repository-configurable) and signed by a different (unofficial-builds) key, so it is trusted
  // over TLS rather than verified against the official release keys.
  if (nodeMirrorBaseUrl === DEFAULT_NODE_MIRROR_BASE_URL) {
    try {
      const muslAssets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl: UNOFFICIAL_NODE_MIRROR_BASE_URL, version, muslOnly: true, verifySignature: false, cacheDir })
      assets.push(...muslAssets)
    } catch {
      // Musl variants may not be available for all Node.js versions (e.g. very old ones)
    }
  }

  return assets
}

async function readNodeAssetsFromMirror (
  fetch: FetchFromRegistry,
  opts: {
    nodeMirrorBaseUrl: string
    version: string
    muslOnly: boolean
    verifySignature: boolean
    cacheDir?: string
  }
): Promise<PlatformAssetResolution[]> {
  const { nodeMirrorBaseUrl, version, muslOnly, verifySignature, cacheDir } = opts
  // The URL is pinned to one released version, which is what makes it
  // eligible for the SHASUMS disk cache.
  const integritiesFileUrl = `${nodeMirrorBaseUrl}v${version}/SHASUMS256.txt`
  const shasumsFileItems = verifySignature
    ? await fetchVerifiedNodeShasumsFileCached(fetch, integritiesFileUrl, { cacheDir })
    : await fetchShasumsFileCached(fetch, integritiesFileUrl, { cacheDir })
  const escaped = version.replace(/\\/g, '\\\\').replace(/\./g, '\\.')
  // The second capture group uses [^.-]+ to stop at a dash, so that the optional
  // third group can capture the '-musl' suffix separately (e.g. 'x64' + '-musl').
  const pattern = new RegExp(`^node-v${escaped}-([^-.]+)-([^.-]+)(-musl)?\\.(?:tar\\.gz|zip)$`)
  const assets: PlatformAssetResolution[] = []
  for (const { integrity, fileName } of shasumsFileItems) {
    const match = pattern.exec(fileName)
    if (!match) continue

    let [, platform, arch, muslSuffix] = match
    if (platform === 'win') {
      platform = 'win32'
    }
    const isMusl = muslSuffix != null
    if (muslOnly && !isMusl) continue

    const libc = isMusl ? 'musl' : undefined
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
      integrity,
      url,
    }
    if (resolution.archive === 'zip') {
      resolution.prefix = address.basename
    }
    const target: PlatformAssetTarget = {
      os: platform,
      cpu: arch,
      ...(libc != null && { libc }),
    }
    assets.push({
      targets: [target],
      resolution,
    })
  }
  return assets
}

interface NodeVersion {
  version: string
  lts: false | string
}

const SEMVER_OPTS = {
  includePrerelease: true,
  loose: true,
}

export async function resolveNodeVersion (
  fetch: FetchFromRegistry,
  versionSpec: string,
  nodeMirrorBaseUrl?: string
): Promise<string | null> {
  const allVersions = await fetchAllVersions(fetch, nodeMirrorBaseUrl)
  versionSpec = normalizeRuntimeSpec(versionSpec)
  if (versionSpec === 'latest') {
    return allVersions[0].version
  }
  const { versions, versionRange } = filterVersions(allVersions, versionSpec)
  return semver.maxSatisfying(versions, versionRange, SEMVER_OPTS) ?? null
}

export async function resolveNodeVersions (
  fetch: FetchFromRegistry,
  versionSpec?: string,
  nodeMirrorBaseUrl?: string
): Promise<string[]> {
  const allVersions = await fetchAllVersions(fetch, nodeMirrorBaseUrl)
  if (versionSpec == null) {
    return allVersions.map(({ version }) => version)
  }
  versionSpec = normalizeRuntimeSpec(versionSpec)
  if (versionSpec === 'latest') {
    return [allVersions[0].version]
  }
  const { versions, versionRange } = filterVersions(allVersions, versionSpec)
  return versions.filter(version => semver.satisfies(version, versionRange, SEMVER_OPTS))
}

function normalizeRuntimeSpec (versionSpec: string): string {
  versionSpec = versionSpec.trim()
  return versionSpec === '' ? 'latest' : versionSpec
}

async function fetchAllVersions (fetch: FetchFromRegistry, nodeMirrorBaseUrl?: string): Promise<NodeVersion[]> {
  const response = await fetch(`${nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'}index.json`)
  return ((await response.json()) as NodeVersion[]).map(({ version, lts }) => ({
    version: version.substring(1),
    lts,
  }))
}

function getNodeBinsForCurrentOS (platform: string = process.platform): Record<string, string> {
  if (platform === 'win32') {
    return { node: 'node.exe' }
  }
  return { node: 'bin/node' }
}

function filterVersions (versions: NodeVersion[], versionSelector: string): { versions: string[], versionRange: string } {
  if (versionSelector === 'lts') {
    return {
      versions: versions
        .filter(({ lts }) => lts !== false)
        .map(({ version }) => version),
      versionRange: '*',
    }
  }
  const vst = versionSelectorType(versionSelector)
  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()
    return {
      versions: versions
        .filter(({ lts }) => typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion)
        .map(({ version }) => version),
      versionRange: '*',
    }
  }
  return {
    versions: versions.map(({ version }) => version),
    versionRange: versionSelector,
  }
}
