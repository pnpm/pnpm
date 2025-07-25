import { getNodeBinLocationForCurrentOS } from '@pnpm/constants'
import { createHash } from '@pnpm/crypto.hash'
import { fetchShasumsFile } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type WantedDependency, type NodeRuntimeResolution, type ResolveResult } from '@pnpm/resolver-base'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'
import { type PkgResolutionId } from '@pnpm/types'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { getNodeMirror } from './getNodeMirror'

export { getNodeMirror, parseEnvSpecifier }

export interface NodeRuntimeResolveResult extends ResolveResult {
  resolution: NodeRuntimeResolution
  resolvedVia: 'nodejs.org'
}

export async function resolveNodeRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    rawConfig: Record<string, string>
    offline?: boolean
  },
  wantedDependency: WantedDependency
): Promise<NodeRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'node' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null
  if (ctx.offline) throw new PnpmError('NO_OFFLINE_NODEJS_RESOLUTION', 'Offline Node.js resolution is not supported')
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:'.length)
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(versionSpec)
  const nodeMirrorBaseUrl = getNodeMirror(ctx.rawConfig, releaseChannel)
  const version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpecifier, nodeMirrorBaseUrl)
  if (!version) {
    throw new PnpmError('NODEJS_VERSION_NOT_FOUND', `Could not find a Node.js version that satisfies ${versionSpec}`)
  }
  const integrities = await loadShasumsFile(ctx.fetchFromRegistry, nodeMirrorBaseUrl, version)
  return {
    id: `node@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'nodejs.org',
    manifest: {
      name: 'node',
      version,
      bin: getNodeBinLocationForCurrentOS(),
    },
    resolution: {
      type: 'nodeRuntime',
      integrities,
    },
  }
}

async function loadShasumsFile (fetch: FetchFromRegistry, nodeMirrorBaseUrl: string, version: string): Promise<Record<string, string>> {
  const integritiesFileUrl = `${nodeMirrorBaseUrl}/v${version}/SHASUMS256.txt`
  const shasumsFileContent = await fetchShasumsFile(fetch, integritiesFileUrl)
  const lines = shasumsFileContent.split('\n')
  const integrities: Record<string, string> = {}
  const escaped = version.replace(/\./g, '\\.');
  const pattern = new RegExp(
    `^node-v${escaped}-([^-.]+)-([^.]+)\\.(?:tar\\.gz|zip)$`,
  )
  for (const line of lines) {
    if (!line) continue
    const [sha256, file] = line.trim().split(/\s+/)

    const match = pattern.exec(file);
    if (!match) continue

    const buffer = Buffer.from(sha256, 'hex')
    const base64 = buffer.toString('base64')
    const integrity = `sha256-${base64}`
    const [, platform, arch] = match
    integrities[`${platform}-${arch}`] = integrity
  }

  return integrities
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
