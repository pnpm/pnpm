import { isIP } from 'node:net'

import type { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching.types'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

export interface NodeVersion {
  version: string
  lts: false | string
}

const SEMVER_OPTS = {
  includePrerelease: true,
  loose: true,
}

const MAX_NODE_MIRROR_REDIRECTS = 20

export interface NodeVersionFetchOptions {
  nodeMirrorBaseUrl?: string
  getAuthHeader?: GetAuthHeader
}

export async function resolveNodeVersion (
  fetch: FetchFromRegistry,
  versionSpec: string,
  opts?: string | NodeVersionFetchOptions
): Promise<string | null> {
  const { nodeMirrorBaseUrl, getAuthHeader } = normalizeNodeVersionFetchOptions(opts)
  const allVersions = await fetchAllVersions(createAuthenticatedFetch(fetch, getAuthHeader), nodeMirrorBaseUrl)
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
  opts?: string | NodeVersionFetchOptions
): Promise<string[]> {
  const { nodeMirrorBaseUrl, getAuthHeader } = normalizeNodeVersionFetchOptions(opts)
  const allVersions = await fetchAllVersions(createAuthenticatedFetch(fetch, getAuthHeader), nodeMirrorBaseUrl)
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

export function normalizeRuntimeSpec (versionSpec: string): string {
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

export function normalizeNodeVersionFetchOptions (opts?: string | NodeVersionFetchOptions): NodeVersionFetchOptions {
  return typeof opts === 'string' ? { nodeMirrorBaseUrl: opts } : opts ?? {}
}

export function createAuthenticatedFetch (fetch: FetchFromRegistry, getAuthHeader?: GetAuthHeader): FetchFromRegistry {
  if (getAuthHeader == null) return fetch
  return async (url, opts) => {
    let currentUrl = url
    for (let redirectCount = 0; ; redirectCount++) {
      // eslint-disable-next-line no-await-in-loop -- each redirect hop needs the previous response's location
      const response = await fetch(currentUrl, {
        ...opts,
        authHeaderValue: getSecureAuthHeader(getAuthHeader, currentUrl),
        redirect: 'manual',
      })
      if (opts?.redirect === 'manual' || !isRedirectStatus(response.status) || redirectCount === MAX_NODE_MIRROR_REDIRECTS) {
        return response
      }
      const location = response.headers.get('location')
      if (location == null) return response
      currentUrl = new URL(location, currentUrl).toString()
    }
  }
}

export function getSecureAuthHeader (getAuthHeader: GetAuthHeader | undefined, url: string): string | undefined {
  const authHeaderValue = getAuthHeader?.(url)
  if (authHeaderValue == null) return undefined
  const parsed = new URL(url)
  if (parsed.protocol === 'https:' || isLoopbackHost(parsed.hostname)) return authHeaderValue
  return undefined
}

function isLoopbackHost (hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' || (isIP(hostname) === 4 && hostname.startsWith('127.'))
}

function isRedirectStatus (status: number): boolean {
  return status === 301 || status === 302 || status === 303 || status === 307 || status === 308
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
