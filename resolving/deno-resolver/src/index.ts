import { getDenoBinLocationForCurrentOS } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import {
  type WantedDependency,
  type PlatformAssetResolution,
  type PlatformAssetTarget,
  type ResolveResult,
  type BinaryResolution,
} from '@pnpm/resolver-base'
import { type PkgResolutionId } from '@pnpm/types'
import { type NpmResolver } from '@pnpm/npm-resolver'
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
  resolution: PlatformAssetResolution[]
  resolvedVia: 'github.com/denoland/deno'
}

export async function resolveDenoRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    rawConfig: Record<string, string>
    offline?: boolean
    resolveFromNpm: NpmResolver
  },
  wantedDependency: WantedDependency
): Promise<DenoRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'deno' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:'.length)
  // We use the npm registry for version resolution as it is easier than using the GitHub API for releases,
  // which uses pagination (e.g. https://api.github.com/repos/denoland/deno/releases?per_page=100).
  const npmResolution = await ctx.resolveFromNpm({ ...wantedDependency, bareSpecifier: versionSpec }, {})
  if (npmResolution == null) {
    throw new PnpmError('DENO_RESOLUTION_FAILURE', `Could not resolve Deno ${wantedDependency.bareSpecifier}`)
  }
  const version = npmResolution.manifest.version
  const res = await ctx.fetchFromRegistry(`https://api.github.com/repos/denoland/deno/releases/tags/v${version}`)
  const data = (await res.json()) as { assets: Array<{ name: string }> }
  const assets: PlatformAssetResolution[] = []
  await Promise.all(data.assets.map(async (asset) => {
    const targets = parseAssetName(asset.name)
    if (!targets) return
    const sha256sumFileUrl = `https://github.com/denoland/deno/releases/download/v${version}/${asset.name}`
    const sha256sumFile = await (await ctx.fetchFromRegistry(sha256sumFileUrl)).text()
    const sha256 = asset.name.includes('windows') ? parseSha256ForWindows(sha256sumFile) : sha256sumFile.trim().split(/\s+/)[0]
    const buffer = Buffer.from(sha256, 'hex')
    const base64 = buffer.toString('base64')
    assets.push({
      targets,
      resolution: {
        type: 'binary',
        url: sha256sumFileUrl.replace(/\.sha256sum$/, ''),
        integrity: `sha256-${base64}`,
        bin: asset.name.includes('windows') ? 'deno.exe' : 'deno',
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
    resolution: assets,
  }
}

function parseAssetName (name: string): PlatformAssetTarget[] | null {
  const m = ASSET_REGEX.exec(name)
  if (!m || !m.groups) return null
  const os = OS_MAP[m.groups.os as keyof typeof OS_MAP]
  const cpu = CPU_MAP[m.groups.cpu as keyof typeof CPU_MAP]
  const targets = [{ os, cpu }]
  if (os === 'win32' && cpu === 'x64') {
    targets.push({ os: 'win32', cpu: 'arm64' })
  }
  return targets
}

function parseSha256ForWindows (block: string): string {
  // ^ start of line, “Hash”, colon, 64 hex chars
  const match = block.match(/^\s*Hash\s*:\s*([A-Fa-f0-9]{64})\b/m)
  if (!match) {
    throw new Error('Hash not found')
  }
  return match[1].toLowerCase()
}
