import { getDenoBinLocationForCurrentOS } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type DenoRuntimeFetcher, type FetchResult } from '@pnpm/fetcher-base'
import { type WantedDependency, type DenoRuntimeResolution, type ResolveResult } from '@pnpm/resolver-base'
import { type PkgResolutionId } from '@pnpm/types'
import { type NpmResolver } from '@pnpm/npm-resolver'
import { addFilesFromDir } from '@pnpm/worker'
import { downloadAndUnpackZip } from '@pnpm/node.fetcher'

export interface DenoRuntimeResolveResult extends ResolveResult {
  resolution: DenoRuntimeResolution
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
  const data = (await res.json()) as { assets: Array<{ name: string }>}
  const artifacts: Array<{ integrity: string, os: string[], cpu: string[], file: string }> = []
  await Promise.all(data.assets.map(async (asset) => {
    let artifact
    switch (asset.name) {
      case 'deno-aarch64-apple-darwin.zip.sha256sum':
        artifact = {
          os: ['darwin'],
          cpu: ['arm64'],
        }
        break
      case 'deno-aarch64-unknown-linux-gnu.zip.sha256sum':
        artifact = {
          os: ['linux'],
          cpu: ['arm64'],
        }
        break
      case 'deno-x86_64-apple-darwin.zip.sha256sum':
        artifact = {
          os: ['darwin'],
          cpu: ['x64'],
        }
        break
      case 'deno-x86_64-pc-windows-msvc.zip.sha256sum':
        artifact = {
          os: ['win32'],
          cpu: ['x64', 'arm64'],
        }
        break
      case 'deno-x86_64-unknown-linux-gnu.zip.sha256sum':
        artifact = {
          os: ['linux'],
          cpu: ['x64'],
        }
        break
    }
    if (!artifact) return
    const sha256sumFile = await (await ctx.fetchFromRegistry(`https://github.com/denoland/deno/releases/download/v${version}/${asset.name}`)).text()
    const sha256 = asset.name.includes('windows') ? parseSha256ForWindows(sha256sumFile) : sha256sumFile.trim().split(/\s+/)[0]
    const buffer = Buffer.from(sha256, 'hex')
    const base64 = buffer.toString('base64')
    artifacts.push({
      ...artifact,
      file: asset.name.replace(/\.sha256sum$/, ''),
      integrity: `sha256-${base64}`,
    })
  }))

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
      type: 'denoRuntime',
      artifacts,
    },
  }
}

function parseSha256ForWindows(block: string): string {
  const match = block.match(
    /^\s*Hash\s*:\s*([A-Fa-f0-9]{64})\b/m   // ^ start of line, “Hash”, colon, 64 hex chars
  )
  if (!match) {
    throw new Error('Hash not found')
  }
  return match[1].toLowerCase()
}

export function createDenoRuntimeFetcher (ctx: {
  fetch: FetchFromRegistry
  rawConfig: Record<string, string>
  offline?: boolean
}): { denoRuntime: DenoRuntimeFetcher } {
  const fetchDenoRuntime: DenoRuntimeFetcher = async (cafs, resolution, opts) => {
    if (!opts.pkg.version) {
      throw new PnpmError('CANNOT_FETCH_DENO_WITHOUT_VERSION', 'Cannot fetch Deno without a version')
    }
    if (ctx.offline) {
      throw new PnpmError('CANNOT_DOWNLOAD_DENO_OFFLINE', 'Cannot download Deno because offline mode is enabled.')
    }
    const version = opts.pkg.version

    const artifact = resolution.artifacts.find((artifact) => artifact.os.includes(process.platform) && artifact.cpu.includes(process.arch))

    if (!artifact) throw new Error('No artifact found for the current system')

    const tempLocation = await cafs.tempDir()
    await downloadAndUnpackZip(ctx.fetch, {
      url: `https://github.com/denoland/deno/releases/download/v${version}/${artifact.file}`,
      integrity: artifact.integrity,
      isZip: true,
      // basename: `deno-${version}`,
      basename: '',
    }, tempLocation)
    const manifest = {
      name: 'deno',
      version,
      bin: 'deno',
    }
    return {
      ...await addFilesFromDir({
        storeDir: cafs.storeDir,
        dir: tempLocation,
        filesIndexFile: opts.filesIndexFile,
        readManifest: false,
      }),
      manifest,
    }
  }
  return {
    denoRuntime: fetchDenoRuntime,
  }
}
