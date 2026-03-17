import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import type {
  BinaryResolution,
  PlatformAssetResolution,
  ResolveOptions,
  ResolveResult,
  VariationsResolution,
  WantedDependency,
} from '@pnpm/resolver-base'
import type { PkgResolutionId } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

import { fetchChecksumFile, resolveGitHubVersion } from './github.js'
import { fetchAquaRegistryPackage, findMatchingOverride } from './registry.js'
import { expandAssets, expandChecksumAssetName } from './template.js'

export interface AquaResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'aqua'
}

export async function resolveAqua (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    offline?: boolean
  },
  wantedDependency: WantedDependency,
  opts?: Partial<ResolveOptions>
): Promise<AquaResolveResult | null> {
  const specifier = wantedDependency.bareSpecifier
  if (!specifier?.startsWith('aqua:')) return null

  if (ctx.offline) {
    throw new PnpmError('AQUA_OFFLINE', 'Cannot resolve aqua packages in offline mode')
  }

  if (opts?.currentPkg && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as VariationsResolution,
      resolvedVia: 'aqua',
    }
  }

  const { owner, repo, versionSpec } = parseAquaSpecifier(specifier)

  // Fetch the aqua registry YAML and resolve the version in parallel
  const [pkg, version] = await Promise.all([
    fetchAquaRegistryPackage(ctx.fetchFromRegistry, owner, repo),
    resolveGitHubVersion(ctx.fetchFromRegistry, owner, repo, versionSpec),
  ])

  // Find the matching version override from the registry
  const override = findMatchingOverride(pkg, version)

  // Expand asset templates for all supported platforms
  const expandedAssets = expandAssets(owner, repo, version, override)

  if (expandedAssets.length === 0) {
    throw new PnpmError(
      'AQUA_NO_ASSETS',
      `No downloadable assets found for ${owner}/${repo}@${version}`
    )
  }

  // Fetch checksums in parallel for all assets that have checksum config
  const checksumsByAsset = await resolveChecksums(
    ctx.fetchFromRegistry,
    owner,
    repo,
    version,
    expandedAssets
  )

  const cleanVersion = version.startsWith('v') ? version.substring(1) : version
  const assets: PlatformAssetResolution[] = []

  for (const expanded of expandedAssets) {
    const sha256Hex = checksumsByAsset.get(expanded.assetName)
    const integrity = sha256Hex
      ? `sha256-${Buffer.from(sha256Hex, 'hex').toString('base64')}`
      : ''

    // Determine the bin path from the files configuration
    const bin = deriveBin(expanded.files, expanded.format)

    const resolution: BinaryResolution = {
      type: 'binary',
      archive: expanded.format === 'zip' ? 'zip' : 'tarball',
      url: expanded.url,
      integrity,
      bin,
    }

    assets.push({
      targets: [expanded.target],
      resolution,
    })
  }

  assets.sort((a, b) => lexCompare(
    (a.resolution as BinaryResolution).url,
    (b.resolution as BinaryResolution).url
  ))

  const pkgName = repo.toLowerCase()

  return {
    id: `${pkgName}@aqua:${cleanVersion}` as PkgResolutionId,
    normalizedBareSpecifier: `aqua:${owner}/${repo}@${cleanVersion}`,
    alias: wantedDependency.alias ?? pkgName,
    resolvedVia: 'aqua',
    manifest: {
      name: pkgName,
      version: cleanVersion,
    },
    resolution: {
      type: 'variations',
      variants: assets,
    },
  }
}

function parseAquaSpecifier (specifier: string): {
  owner: string
  repo: string
  versionSpec?: string
} {
  // "aqua:owner/repo" or "aqua:owner/repo@version"
  const rest = specifier.substring('aqua:'.length)
  const atIdx = rest.lastIndexOf('@')

  let ownerRepo: string
  let versionSpec: string | undefined
  if (atIdx > 0) {
    ownerRepo = rest.substring(0, atIdx)
    versionSpec = rest.substring(atIdx + 1)
  } else {
    ownerRepo = rest
  }

  const slashIdx = ownerRepo.indexOf('/')
  if (slashIdx <= 0) {
    throw new PnpmError(
      'AQUA_INVALID_SPECIFIER',
      `Invalid aqua specifier "${specifier}". Expected format: aqua:owner/repo[@version]`
    )
  }

  return {
    owner: ownerRepo.substring(0, slashIdx),
    repo: ownerRepo.substring(slashIdx + 1),
    versionSpec,
  }
}

async function resolveChecksums (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string,
  version: string,
  expandedAssets: Array<{ assetName: string, checksum?: { asset: string, algorithm: string } }>
): Promise<Map<string, string>> {
  const result = new Map<string, string>()

  // Group assets by checksum file to avoid fetching the same file multiple times
  const checksumFiles = new Map<string, string[]>()
  for (const asset of expandedAssets) {
    if (!asset.checksum) continue
    const checksumAssetName = expandChecksumAssetName(
      asset.checksum.asset,
      asset.assetName,
      version
    )
    let assetNames = checksumFiles.get(checksumAssetName)
    if (!assetNames) {
      assetNames = []
      checksumFiles.set(checksumAssetName, assetNames)
    }
    assetNames.push(asset.assetName)
  }

  const fetchPromises = [...checksumFiles.entries()].map(
    async ([checksumFileName, assetNames]) => {
      const checksums = await fetchChecksumFile(
        fetchFromRegistry,
        owner,
        repo,
        version,
        checksumFileName
      )
      for (const assetName of assetNames) {
        // Try exact match first, then just the hash (for single-asset checksum files)
        const hash = checksums.get(assetName) ?? checksums.get('')
        if (hash) {
          result.set(assetName, hash)
        }
      }
    }
  )

  await Promise.all(fetchPromises)
  return result
}

function deriveBin (
  files: Array<{ name: string, src?: string }>,
  format: string
): string | Record<string, string> {
  if (files.length === 0) return ''

  if (files.length === 1) {
    const file = files[0]
    // If there's a src path, use it as the bin path (relative to extraction root).
    // For tarballs, the first path component is stripped during extraction.
    if (file.src) {
      return stripFirstPathComponent(file.src, format)
    }
    return file.name
  }

  // Multiple files: create a name → path mapping
  const bin: Record<string, string> = {}
  for (const file of files) {
    if (file.src) {
      bin[file.name] = stripFirstPathComponent(file.src, format)
    } else {
      bin[file.name] = file.name
    }
  }
  return bin
}

function stripFirstPathComponent (filePath: string, format: string): string {
  // pnpm's tarball fetcher strips the first path component for tarballs
  if (format !== 'tar.gz' && format !== 'tgz' && format !== 'tar.bz2' && format !== 'tar.xz') {
    return filePath
  }
  const parts = filePath.split('/')
  if (parts.length > 1) {
    return parts.slice(1).join('/')
  }
  return filePath
}
