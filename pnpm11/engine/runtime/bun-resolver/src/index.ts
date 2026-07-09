import { fetchShasumsFile } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE, type NpmResolver } from '@pnpm/resolving.npm-resolver'
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
import { lexCompare } from '@pnpm/util.lex-comparator'

export interface BunRuntimeResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'github.com/oven-sh/bun'
}

export async function resolveBunRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    offline?: boolean
    resolveFromNpm: NpmResolver
  },
  wantedDependency: WantedDependency,
  opts?: Partial<ResolveOptions>
): Promise<BunRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'bun' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null

  if (opts?.currentPkg && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as VariationsResolution,
      resolvedVia: 'github.com/oven-sh/bun',
    }
  }

  const versionSpec = normalizeRuntimeSpec(wantedDependency.bareSpecifier.substring('runtime:'.length))
  // We use the npm registry for version resolution as it is easier than using the GitHub API for releases,
  // which uses pagination (e.g. https://api.github.com/repos/oven-sh/bun/releases?per_page=100).
  const npmResolution = await ctx.resolveFromNpm({ ...wantedDependency, bareSpecifier: versionSpec }, {})
  if (npmResolution == null) {
    throw new PnpmError('BUN_RESOLUTION_FAILURE', `Could not resolve Bun version specified as ${versionSpec}`)
  }
  const version = npmResolution.manifest.version
  const assets = await readBunAssets(ctx.fetchFromRegistry, version)
  assets.sort((asset1, asset2) => lexCompare((asset1.resolution as BinaryResolution).url, (asset2.resolution as BinaryResolution).url))

  return {
    id: `bun@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'github.com/oven-sh/bun',
    manifest: {
      name: 'bun',
      version,
      bin: getBunBinLocationForCurrentOS(),
    },
    resolution: {
      type: 'variations',
      variants: assets,
    },
  }
}

export async function resolveLatestBunRuntime (
  ctx: { resolveFromNpm: NpmResolver },
  query: LatestQuery,
  opts: ResolveOptions
): Promise<LatestInfo | undefined> {
  const manifestSpec = query.wantedDependency.bareSpecifier
  if (query.wantedDependency.alias !== 'bun' || !manifestSpec?.startsWith('runtime:')) return undefined
  const versionSpec = query.compatible ? normalizeRuntimeSpec(manifestSpec.substring('runtime:'.length)) : 'latest'
  try {
    const npmResolution = await ctx.resolveFromNpm(
      { alias: 'bun', bareSpecifier: versionSpec },
      query.compatible ? opts : { ...opts, update: 'latest' }
    )
    if (npmResolution?.policyViolation?.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE) return {}
    if (!npmResolution?.manifest) return {}
    return { latestManifest: { name: 'bun', version: npmResolution.manifest.version } }
  } catch (err) {
    if (opts.publishedBy && (err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION') {
      return {}
    }
    throw err
  }
}

function normalizeRuntimeSpec (versionSpec: string): string {
  versionSpec = versionSpec.trim()
  return versionSpec === '' ? 'latest' : versionSpec
}

async function readBunAssets (fetch: FetchFromRegistry, version: string): Promise<PlatformAssetResolution[]> {
  const integritiesFileUrl = `https://github.com/oven-sh/bun/releases/download/bun-v${version}/SHASUMS256.txt`
  const shasumsFileItems = await fetchShasumsFile(fetch, integritiesFileUrl)
  const pattern = /^bun-([^-.]+)-([^-.]+)(-musl)?\.zip$/
  const assets: PlatformAssetResolution[] = []
  for (const { integrity, fileName } of shasumsFileItems) {
    const match = pattern.exec(fileName)
    if (!match) continue

    let [, platform, arch, musl] = match
    if (platform === 'windows') {
      platform = 'win32'
    }
    if (arch === 'aarch64') {
      arch = 'arm64'
    }
    const url = `https://github.com/oven-sh/bun/releases/download/bun-v${version}/${fileName}`
    const resolution: BinaryResolution = {
      type: 'binary',
      archive: 'zip',
      bin: getBunBinLocationForCurrentOS(platform),
      integrity,
      url,
      prefix: fileName.replace(/\.zip$/, ''),
    }
    const target: PlatformAssetTarget = {
      os: platform,
      cpu: arch,
    }
    if (musl != null) {
      target.libc = 'musl'
    }
    assets.push({
      targets: [target],
      resolution,
    })
  }
  return assets
}

function getBunBinLocationForCurrentOS (platform: string = process.platform): string {
  return platform === 'win32' ? 'bun.exe' : 'bun'
}
