import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import type { BinaryResolution, PlatformAssetResolution, PlatformAssetTarget } from '@pnpm/resolver-base'

interface HomebrewFormula {
  name: string
  full_name: string
  aliases: string[]
  versions: {
    stable: string
    bottle: boolean
  }
  bottle: {
    stable: {
      root_url: string
      files: Record<string, {
        url: string
        sha256: string
        cellar?: string
      }>
    }
  }
  dependencies: string[]
  build_dependencies: string[]
}

const HOMEBREW_PLATFORM_MAP: Record<string, PlatformAssetTarget> = {}

// macOS arm64 versions
for (const v of ['arm64_tahoe', 'arm64_sequoia', 'arm64_sonoma', 'arm64_ventura', 'arm64_monterey']) {
  HOMEBREW_PLATFORM_MAP[v] = { os: 'darwin', cpu: 'arm64' }
}

// macOS x64 versions
for (const v of ['tahoe', 'sequoia', 'sonoma', 'ventura', 'monterey']) {
  HOMEBREW_PLATFORM_MAP[v] = { os: 'darwin', cpu: 'x64' }
}

// Linux
HOMEBREW_PLATFORM_MAP['x86_64_linux'] = { os: 'linux', cpu: 'x64' }
HOMEBREW_PLATFORM_MAP['arm64_linux'] = { os: 'linux', cpu: 'arm64' }

export interface HomebrewResolveResult {
  version: string
  assets: PlatformAssetResolution[]
  dependencies: string[]
}

export async function resolveFromHomebrew (
  fetchFromRegistry: FetchFromRegistry,
  name: string,
  _versionSpec?: string
): Promise<HomebrewResolveResult> {
  const formula = await fetchHomebrewFormula(fetchFromRegistry, name)

  if (!formula.versions.bottle) {
    throw new PnpmError('BROOP_NO_BOTTLE', `Homebrew formula "${name}" does not have pre-built bottles`)
  }

  const version = formula.versions.stable
  const formulaSlug = formula.full_name.replace(/\+/g, 'x').replace(/\//g, '--')

  // Get anonymous GHCR token (one token works for all platforms of the same formula)
  const token = await getGhcrToken(fetchFromRegistry, formulaSlug)

  // Resolve bottle URLs in parallel
  const entries = Object.entries(formula.bottle.stable.files)
  const resolvedUrls = await Promise.all(
    entries.map(([, file]) => resolveGhcrBlobUrl(fetchFromRegistry, file.url, token))
  )

  const assets: PlatformAssetResolution[] = []
  const seenTargets = new Set<string>()

  for (let i = 0; i < entries.length; i++) {
    const [platform, file] = entries[i]
    const target = HOMEBREW_PLATFORM_MAP[platform]
    if (!target) continue

    // Deduplicate: multiple macOS versions map to the same os/cpu
    const targetKey = `${target.os}-${target.cpu}`
    if (seenTargets.has(targetKey)) continue
    seenTargets.add(targetKey)

    const downloadUrl = resolvedUrls[i]
    const base64Hash = Buffer.from(file.sha256, 'hex').toString('base64')

    // Determine bin name: prefer first alias over formula name.
    // In Homebrew, aliases often correspond to the actual binary name
    // (e.g., ripgrep has alias "rg" which is the binary name).
    const binName = formula.aliases[0] ?? formula.name

    // Homebrew bottles extract to: {formula_name}/{version}/bin/{binary}
    // pnpm's tarball fetcher strips the first path component,
    // so the bin path after extraction is: {version}/bin/{binName}
    const resolution: BinaryResolution = {
      type: 'binary',
      archive: 'tarball',
      url: downloadUrl,
      integrity: `sha256-${base64Hash}`,
      bin: `${version}/bin/${binName}`,
    }

    assets.push({
      targets: [target],
      resolution,
    })
  }

  if (assets.length === 0) {
    throw new PnpmError('BROOP_NO_PLATFORMS', `No supported platforms found for Homebrew formula "${name}"`)
  }

  return {
    version,
    assets,
    dependencies: formula.dependencies,
  }
}

async function fetchHomebrewFormula (
  fetchFromRegistry: FetchFromRegistry,
  name: string
): Promise<HomebrewFormula> {
  const url = `https://formulae.brew.sh/api/formula/${encodeURIComponent(name)}.json`
  const res = await fetchFromRegistry(url)
  if (!res.ok) {
    throw new PnpmError(
      'BROOP_HOMEBREW_FETCH',
      `Failed to fetch Homebrew formula "${name}": ${res.status} ${res.statusText}`
    )
  }
  return await res.json() as HomebrewFormula
}

async function getGhcrToken (
  fetchFromRegistry: FetchFromRegistry,
  formulaSlug: string
): Promise<string> {
  const tokenUrl = `https://ghcr.io/token?scope=${encodeURIComponent(`repository:homebrew/core/${formulaSlug}:pull`)}&service=ghcr.io`
  const res = await fetchFromRegistry(tokenUrl)
  if (!res.ok) {
    throw new PnpmError('BROOP_GHCR_TOKEN', `Failed to get GHCR token for "${formulaSlug}": ${res.status}`)
  }
  const data = await res.json() as { token: string }
  return data.token
}

async function resolveGhcrBlobUrl (
  fetchFromRegistry: FetchFromRegistry,
  ghcrBlobUrl: string,
  token: string
): Promise<string> {
  // Request the blob with redirect: manual to capture the CDN URL
  const res = await fetchFromRegistry(ghcrBlobUrl, {
    redirect: 'manual',
    authHeaderValue: `Bearer ${token}`,
  })

  const location = res.headers.get('location')
  if (location) {
    return location
  }

  // If no redirect, the response contains the blob directly.
  // Return the original URL — the fetcher will need auth.
  return ghcrBlobUrl
}
