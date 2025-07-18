import { createHash } from '@pnpm/crypto.hash'
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
  const { versionIntegrity: integrity, shasumsFileContent } = await loadShasumsFile(ctx.fetchFromRegistry, nodeMirrorBaseUrl, version)
  return {
    id: `node@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'nodejs.org',
    manifest: {
      name: 'node',
      version,
      bin: process.platform === 'win32' ? 'node.exe' : 'bin/node',
    },
    resolution: {
      type: 'nodeRuntime',
      integrity,
      _shasumsFileContent: shasumsFileContent,
    },
  }
}

async function loadShasumsFile (fetch: FetchFromRegistry, nodeMirrorBaseUrl: string, version: string): Promise<{
  shasumsFileContent: string
  versionIntegrity: string
}> {
  const integritiesFileUrl = `${nodeMirrorBaseUrl}/v${version}/SHASUMS256.txt`
  const res = await fetch(integritiesFileUrl)
  if (!res.ok) {
    throw new PnpmError(
      'NODE_FETCH_INTEGRITY_FAILED',
      `Failed to fetch integrity file: ${integritiesFileUrl} (status: ${res.status})`
    )
  }

  const shasumsFileContent = await res.text()
  const versionIntegrity = createHash(shasumsFileContent)

  return {
    shasumsFileContent,
    versionIntegrity,
  }
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
