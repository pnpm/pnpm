import { getBunBinLocationForCurrentOS } from '@pnpm/constants'
import { fetchShasumsFile } from '@pnpm/crypto.shasums-file'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import {
  type BinaryResolution,
  type PlatformAssetResolution,
  type PlatformAssetTarget,
  type ResolveResult,
  type VariationsResolution,
  type WantedDependency,
} from '@pnpm/resolver-base'
import { type PkgResolutionId } from '@pnpm/types'
import { type NpmResolver } from '@pnpm/npm-resolver'
import { lexCompare } from '@pnpm/util.lex-comparator'

export interface BunRuntimeResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'github.com/oven-sh/bun'
}

export async function resolveBunRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    rawConfig: Record<string, string>
    offline?: boolean
    resolveFromNpm: NpmResolver
  },
  wantedDependency: WantedDependency
): Promise<BunRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'bun' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:'.length)
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
