import { getDenoBinLocationForCurrentOS } from '@pnpm/constants'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import {
  type WantedDependency,
  type PlatformAssetResolution,
  type PlatformAssetTarget,
  type ResolveResult,
  type ZipResolution,
} from '@pnpm/resolver-base'
import { type PkgResolutionId } from '@pnpm/types'
import { type NpmResolver } from '@pnpm/npm-resolver'
import { lexCompare } from '@pnpm/util.lex-comparator'

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
  if (npmResolution == null) throw new Error('Could not resolve')
  const version = npmResolution.manifest.version
  const res = await ctx.fetchFromRegistry(`https://api.github.com/repos/denoland/deno/releases/tags/v${version}`)
  const data = (await res.json()) as { assets: Array<{ name: string }> }
  const artifacts: Array<PlatformAssetResolution> = []
  await Promise.all(data.assets.map(async (asset) => {
    let targets: PlatformAssetTarget[] | undefined = undefined
    switch (asset.name) {
    case 'deno-aarch64-apple-darwin.zip.sha256sum':
      targets = [{
        os: 'darwin',
        cpu: 'arm64',
      }]
      break
    case 'deno-aarch64-unknown-linux-gnu.zip.sha256sum':
      targets = [{
        os: 'linux',
        cpu: 'arm64',
      }]
      break
    case 'deno-x86_64-apple-darwin.zip.sha256sum':
      targets = [{
        os: 'darwin',
        cpu: 'x64',
      }]
      break
    case 'deno-x86_64-pc-windows-msvc.zip.sha256sum':
      targets = [{
        os: 'win32',
        cpu: 'x64',
      }, {
        os: 'win32',
        cpu: 'arm64',
      }]
      break
    case 'deno-x86_64-unknown-linux-gnu.zip.sha256sum':
      targets = [{
        os: 'linux',
        cpu: 'x64',
      }]
      break
    }
    if (!targets) return
    const sha256sumFileUrl = `https://github.com/denoland/deno/releases/download/v${version}/${asset.name}`
    const sha256sumFile = await (await ctx.fetchFromRegistry(sha256sumFileUrl)).text()
    const sha256 = asset.name.includes('windows') ? parseSha256ForWindows(sha256sumFile) : sha256sumFile.trim().split(/\s+/)[0]
    const buffer = Buffer.from(sha256, 'hex')
    const base64 = buffer.toString('base64')
    artifacts.push({
      targets,
      resolution: {
        type: 'zip',
        url: sha256sumFileUrl.replace(/\.sha256sum$/, ''),
        integrity: `sha256-${base64}`,
      },
    })
  }))
  artifacts.sort((artifact1, artifact2) => lexCompare((artifact1.resolution as ZipResolution).url, (artifact2.resolution as ZipResolution).url))

  return {
    id: `deno@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'github.com/denoland/deno',
    manifest: {
      name: 'deno',
      version,
      bin: getDenoBinLocationForCurrentOS(),
    },
    resolution: artifacts,
  }
}

function parseSha256ForWindows (block: string): string {
  // ^ start of line, “Hash”, colon, 64 hex chars
  const match = block.match(/^\s*Hash\s*:\s*([A-Fa-f0-9]{64})\b/m)
  if (!match) {
    throw new Error('Hash not found')
  }
  return match[1].toLowerCase()
}
