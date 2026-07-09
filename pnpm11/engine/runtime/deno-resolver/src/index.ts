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

const ASSET_REGEX = /^deno-(?<cpu>aarch64|x86_64)-(?<os>apple-darwin|unknown-linux-gnu|pc-windows-msvc)\.zip\.sha256sum$/
const OS_MAP = {
  'apple-darwin': 'darwin',
  'unknown-linux-gnu': 'linux',
  'pc-windows-msvc': 'win32',
} as const
const CPU_MAP = {
  aarch64: 'arm64',
  x86_64: 'x64',
} as const

export interface DenoRuntimeResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'github.com/denoland/deno'
}

export async function resolveDenoRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    offline?: boolean
    resolveFromNpm: NpmResolver
  },
  wantedDependency: WantedDependency,
  opts?: Partial<ResolveOptions>
): Promise<DenoRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'deno' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null

  if (opts?.currentPkg && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as VariationsResolution,
      resolvedVia: 'github.com/denoland/deno',
    }
  }

  const versionSpec = normalizeRuntimeSpec(wantedDependency.bareSpecifier.substring('runtime:'.length))
  // We use the npm registry for version resolution as it is easier than using the GitHub API for releases,
  // which uses pagination (e.g. https://api.github.com/repos/denoland/deno/releases?per_page=100).
  const npmResolution = await ctx.resolveFromNpm({ ...wantedDependency, bareSpecifier: versionSpec }, {})
  if (npmResolution == null) {
    throw new PnpmError('DENO_RESOLUTION_FAILURE', `Could not resolve Deno version specified as ${versionSpec}`)
  }
  const version = npmResolution.manifest.version
  const res = await ctx.fetchFromRegistry(`https://api.github.com/repos/denoland/deno/releases/tags/v${version}`)
  const data = (await res.json()) as { assets: Array<{ name: string, browser_download_url: string }> }
  const assets: PlatformAssetResolution[] = []
  if (data.assets == null) {
    throw new PnpmError('DENO_MISSING_ASSETS', `No assets found for Deno v${version}`)
  }
  await Promise.all(data.assets.map(async (asset) => {
    const targets = parseAssetName(asset.name)
    if (!targets) return
    const sha256 = await fetchSha256(ctx.fetchFromRegistry, asset.browser_download_url)
    const base64 = Buffer.from(sha256, 'hex').toString('base64')
    assets.push({
      targets,
      resolution: {
        type: 'binary',
        url: asset.browser_download_url.replace(/\.sha256sum$/, ''),
        integrity: `sha256-${base64}`,
        bin: getDenoBinLocationForCurrentOS(targets[0].os),
        archive: 'zip',
      },
    })
  }))
  assets.sort((asset1, asset2) => lexCompare((asset1.resolution as BinaryResolution).url, (asset2.resolution as BinaryResolution).url))

  return {
    id: `deno@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'github.com/denoland/deno',
    manifest: {
      name: 'deno',
      version,
      bin: getDenoBinLocationForCurrentOS(),
    },
    resolution: {
      type: 'variations',
      variants: assets,
    },
  }
}

export async function resolveLatestDenoRuntime (
  ctx: { resolveFromNpm: NpmResolver },
  query: LatestQuery,
  opts: ResolveOptions
): Promise<LatestInfo | undefined> {
  const manifestSpec = query.wantedDependency.bareSpecifier
  if (query.wantedDependency.alias !== 'deno' || !manifestSpec?.startsWith('runtime:')) return undefined
  const versionSpec = query.compatible ? normalizeRuntimeSpec(manifestSpec.substring('runtime:'.length)) : 'latest'
  try {
    const npmResolution = await ctx.resolveFromNpm(
      { alias: 'deno', bareSpecifier: versionSpec },
      query.compatible ? opts : { ...opts, update: 'latest' }
    )
    if (npmResolution?.policyViolation?.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE) return {}
    if (!npmResolution?.manifest) return {}
    return { latestManifest: { name: 'deno', version: npmResolution.manifest.version } }
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

function parseAssetName (name: string): PlatformAssetTarget[] | null {
  const m = ASSET_REGEX.exec(name)
  if (!m?.groups) return null
  const os = OS_MAP[m.groups.os as keyof typeof OS_MAP]
  const cpu = CPU_MAP[m.groups.cpu as keyof typeof CPU_MAP]
  const targets = [{ os, cpu }]
  if (os === 'win32' && cpu === 'x64') {
    // The Windows x64 binaries of Deno are compatible with arm64 architecture.
    targets.push({ os: 'win32', cpu: 'arm64' })
  }
  return targets
}

function getDenoBinLocationForCurrentOS (platform: string = process.platform): string {
  return platform === 'win32' ? 'deno.exe' : 'deno'
}

async function fetchSha256 (fetch: FetchFromRegistry, url: string): Promise<string> {
  const response = await fetch(url)
  if (!response.ok) {
    throw new PnpmError('DENO_GITHUB_FAILURE', `Failed to GET sha256 at ${url}`)
  }
  const txt = await response.text()
  const m = txt.match(/([a-f0-9]{64})/i)
  if (!m) {
    throw new PnpmError('DENO_PARSE_HASH', `No SHA256 in ${url}`)
  }
  return m[1].toLowerCase()
}
