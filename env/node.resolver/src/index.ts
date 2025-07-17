import { createHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type WantedDependency, type NodeRuntimeResolution, type ResolveResult } from '@pnpm/resolver-base'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'
import { type PkgResolutionId } from '@pnpm/types'

export interface NodeRuntimeResolveResult extends ResolveResult {
  resolution: NodeRuntimeResolution
  resolvedVia: 'nodejs.org'
}

const DEFAULT_NODE_MIRROR_BASE_URL = 'https://nodejs.org/download/release/'

export async function resolveNodeRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    nodeMirrorBaseUrl?: string
  },
  wantedDependency: WantedDependency
): Promise<NodeRuntimeResolveResult | null> {
  if (!wantedDependency.bareSpecifier?.startsWith('runtime:node@')) return null
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:node@'.length)
  const nodeMirrorBaseUrl = ctx.nodeMirrorBaseUrl ?? DEFAULT_NODE_MIRROR_BASE_URL
  const version = await resolveNodeVersion(ctx.fetchFromRegistry, versionSpec, nodeMirrorBaseUrl)
  if (!version) {
    throw new Error('xxx')
  }
  const { versionIntegrity: integrity, body } = await loadShasumsFile(ctx.fetchFromRegistry, nodeMirrorBaseUrl, version)
  return {
    id: `node@runtime:${version}` as PkgResolutionId,
    resolvedVia: 'nodejs.org',
    manifest: {
      name: 'node',
      version,
      bin: process.platform === 'win32' ? 'node.exe' : 'bin/node',
    },
    resolution: {
      type: 'nodeRuntime',
      integrity,
      body,
    },
  }
}

async function loadShasumsFile (fetch: FetchFromRegistry, nodeMirrorBaseUrl: string, version: string) {
  const integritiesFileUrl = `${nodeMirrorBaseUrl}/v${version}/SHASUMS256.txt`
  const res = await fetch(integritiesFileUrl)
  if (!res.ok) {
    throw new PnpmError(
      'NODE_FETCH_INTEGRITY_FAILED',
      `Failed to fetch integrity file: ${integritiesFileUrl} (status: ${res.status})`
    )
  }

  const body = await res.text()
  const versionIntegrity = createHash(body)

  return {
    body,
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
