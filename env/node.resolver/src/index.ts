import { getNodeBinsForCurrentOS } from '@pnpm/constants'
import { fetchShasumsFile } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import {
  type BinaryResolution,
  type PlatformAssetResolution,
  type PlatformAssetTarget,
  type ResolveOptions,
  type ResolveResult,
  type VariationsResolution,
  type WantedDependency,
} from '@pnpm/resolver-base'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'
import { type PkgResolutionId } from '@pnpm/types'
import { parseNodeSpecifier } from './parseNodeSpecifier.js'
import { getNodeMirror } from './getNodeMirror.js'
import { getNodeArtifactAddress } from './getNodeArtifactAddress.js'

export { getNodeMirror, parseNodeSpecifier, getNodeArtifactAddress }

export const DEFAULT_NODE_MIRROR_BASE_URL = 'https://nodejs.org/download/release/'
export const UNOFFICIAL_NODE_MIRROR_BASE_URL = 'https://unofficial-builds.nodejs.org/download/release/'

export interface NodeRuntimeResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'nodejs.org'
}

export async function resolveNodeRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    rawConfig: Record<string, string>
    offline?: boolean
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
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:'.length)
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.rawConfig, releaseChannel)
  const version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpecifier, nodeMirrorBaseUrl)
  if (!version) {
    throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
  }
  const variants = await readNodeAssets(ctx.fetchFromRegistry, nodeMirrorBaseUrl, version)
  const range = version === versionSpec ? version : `^${version}`
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

async function readNodeAssets (fetch: FetchFromRegistry, nodeMirrorBaseUrl: string, version: string): Promise<PlatformAssetResolution[]> {
  const assets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl, version, muslOnly: false })

  // When using the default mirror, also fetch musl variants from unofficial-builds.nodejs.org,
  // since musl builds are not available on the official mirror.
  if (nodeMirrorBaseUrl === DEFAULT_NODE_MIRROR_BASE_URL) {
    try {
      const muslAssets = await readNodeAssetsFromMirror(fetch, { nodeMirrorBaseUrl: UNOFFICIAL_NODE_MIRROR_BASE_URL, version, muslOnly: true })
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
  }
): Promise<PlatformAssetResolution[]> {
  const { nodeMirrorBaseUrl, version, muslOnly } = opts
  const integritiesFileUrl = `${nodeMirrorBaseUrl}v${version}/SHASUMS256.txt`
  const shasumsFileItems = await fetchShasumsFile(fetch, integritiesFileUrl)
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
  if (!versionSpec) {
    return allVersions.map(({ version }) => version)
  }
  if (versionSpec === 'latest') {
    return [allVersions[0].version]
  }
  const { versions, versionRange } = filterVersions(allVersions, versionSpec)
  return versions.filter(version => semver.satisfies(version, versionRange, SEMVER_OPTS))
}

async function fetchAllVersions (fetch: FetchFromRegistry, nodeMirrorBaseUrl?: string): Promise<NodeVersion[]> {
  const response = await fetch(`${nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'}index.json`)
  return ((await response.json()) as NodeVersion[]).map(({ version, lts }) => ({
    version: version.substring(1),
    lts,
  }))
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
