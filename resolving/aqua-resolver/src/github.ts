import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import semver from 'semver'

const GITHUB_API = 'https://api.github.com'

interface GitHubRelease {
  tag_name: string
  prerelease: boolean
  draft: boolean
}

export async function resolveGitHubVersion (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string,
  versionSpec?: string
): Promise<string> {
  if (versionSpec) {
    // If a specific version is requested, check if it exists
    const tag = versionSpec.startsWith('v') ? versionSpec : `v${versionSpec}`
    const exists = await checkReleaseExists(fetchFromRegistry, owner, repo, tag)
    if (exists) return tag
    // Try without 'v' prefix
    if (!versionSpec.startsWith('v')) {
      const existsNoV = await checkReleaseExists(fetchFromRegistry, owner, repo, versionSpec)
      if (existsNoV) return versionSpec
    }
    throw new PnpmError(
      'AQUA_VERSION_NOT_FOUND',
      `Version "${versionSpec}" not found for ${owner}/${repo}`
    )
  }

  // Get latest release
  const url = `${GITHUB_API}/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/latest`
  const res = await fetchFromRegistry(url, { headers: githubHeaders() })
  if (!res.ok) {
    throw new PnpmError(
      'AQUA_GITHUB_FETCH',
      `Failed to fetch latest release for ${owner}/${repo}: ${res.status} ${res.statusText}`
    )
  }
  const release = await res.json() as GitHubRelease
  return release.tag_name
}

export async function listGitHubVersions (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string
): Promise<string[]> {
  const url = `${GITHUB_API}/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases?per_page=100`
  const res = await fetchFromRegistry(url, { headers: githubHeaders() })
  if (!res.ok) {
    throw new PnpmError(
      'AQUA_GITHUB_FETCH',
      `Failed to list releases for ${owner}/${repo}: ${res.status} ${res.statusText}`
    )
  }
  const releases = await res.json() as GitHubRelease[]
  return releases
    .filter((r) => !r.draft && !r.prerelease)
    .map((r) => r.tag_name)
}

export async function fetchChecksumFile (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string,
  tag: string,
  checksumAssetName: string
): Promise<Map<string, string>> {
  const url = `https://github.com/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/download/${encodeURIComponent(tag)}/${checksumAssetName}`
  const res = await fetchFromRegistry(url)
  if (!res.ok) {
    return new Map()
  }
  const text = await res.text()
  return parseChecksumFile(text)
}

function parseChecksumFile (text: string): Map<string, string> {
  const map = new Map<string, string>()
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    // Format: "hash  filename" or "hash filename"
    const parts = trimmed.split(/\s+/)
    if (parts.length >= 2) {
      map.set(parts[parts.length - 1], parts[0])
    } else if (parts.length === 1) {
      // Single hash (e.g., .sha256 file for a single asset)
      map.set('', parts[0])
    }
  }
  return map
}

export function pickBestVersion (
  versions: string[],
  range: string
): string | undefined {
  const clean = versions
    .map((v) => ({ raw: v, semver: semver.clean(v) ?? semver.clean(v.replace(/^v/, '')) }))
    .filter((v): v is { raw: string, semver: string } => v.semver != null)

  clean.sort((a, b) => semver.rcompare(a.semver, b.semver))

  for (const v of clean) {
    if (semver.satisfies(v.semver, range, { includePrerelease: true })) {
      return v.raw
    }
  }
  return undefined
}

async function checkReleaseExists (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string,
  tag: string
): Promise<boolean> {
  const url = `${GITHUB_API}/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/tags/${encodeURIComponent(tag)}`
  const res = await fetchFromRegistry(url, { headers: githubHeaders() })
  return res.ok
}

function githubHeaders (): Record<string, string> {
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
  }
  // Support GITHUB_TOKEN for higher rate limits
  const token = process.env.GITHUB_TOKEN
  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }
  return headers
}
