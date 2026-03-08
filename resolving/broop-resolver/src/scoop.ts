import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import type { BinaryResolution, PlatformAssetResolution, PlatformAssetTarget } from '@pnpm/resolver-base'

interface ScoopManifest {
  version: string
  architecture?: Record<string, ScoopArch | undefined>
  url?: string | string[]
  hash?: string | string[]
  bin?: ScoopBin
  extract_dir?: string
  depends?: string | string[]
}

interface ScoopArch {
  url: string | string[]
  hash?: string | string[]
  bin?: ScoopBin
  extract_dir?: string
}

type ScoopBin = string | Array<string | [string, string]>

const SCOOP_ARCH_MAP: Record<string, PlatformAssetTarget> = {
  '64bit': { os: 'win32', cpu: 'x64' },
  '32bit': { os: 'win32', cpu: 'ia32' },
  'arm64': { os: 'win32', cpu: 'arm64' },
}

const SCOOP_BUCKETS = [
  'https://raw.githubusercontent.com/ScoopInstaller/Main/master/bucket',
  'https://raw.githubusercontent.com/ScoopInstaller/Extras/master/bucket',
]

export interface ScoopResolveResult {
  version: string
  assets: PlatformAssetResolution[]
  dependencies: string[]
}

export async function resolveFromScoop (
  fetchFromRegistry: FetchFromRegistry,
  name: string,
  _versionSpec?: string
): Promise<ScoopResolveResult> {
  const manifest = await fetchScoopManifest(fetchFromRegistry, name)
  const version = manifest.version

  const assets: PlatformAssetResolution[] = []

  if (manifest.architecture) {
    for (const [arch, archData] of Object.entries(manifest.architecture)) {
      const target = SCOOP_ARCH_MAP[arch]
      if (!target || !archData) continue

      const asset = buildScoopAsset(target, archData, manifest)
      if (asset) {
        assets.push(asset)
      }
    }
  } else if (manifest.url) {
    // No architecture-specific entries — assume win32/x64
    const asset = buildScoopAsset(
      { os: 'win32', cpu: 'x64' },
      {
        url: manifest.url,
        hash: manifest.hash,
        bin: manifest.bin,
        extract_dir: manifest.extract_dir,
      },
      manifest
    )
    if (asset) {
      assets.push(asset)
    }
  }

  if (assets.length === 0) {
    throw new PnpmError('BROOP_SCOOP_NO_ASSETS', `No downloadable assets found for Scoop package "${name}"`)
  }

  const dependencies = manifest.depends
    ? (Array.isArray(manifest.depends) ? manifest.depends : [manifest.depends])
    : []

  return { version, assets, dependencies }
}

function buildScoopAsset (
  target: PlatformAssetTarget,
  archData: ScoopArch,
  manifest: ScoopManifest
): PlatformAssetResolution | null {
  const urls = Array.isArray(archData.url) ? archData.url : [archData.url]
  const hashes = archData.hash
    ? (Array.isArray(archData.hash) ? archData.hash : [archData.hash])
    : []

  const url = urls[0]
  if (!url) return null

  const hash = hashes[0]
  const bin = parseScoopBin(archData.bin ?? manifest.bin)
  const extractDir = archData.extract_dir ?? manifest.extract_dir

  const resolution: BinaryResolution = {
    type: 'binary',
    archive: isTarball(url) ? 'tarball' : 'zip',
    url,
    integrity: hash ? `sha256-${Buffer.from(hash, 'hex').toString('base64')}` : '',
    bin,
    ...(extractDir != null && { prefix: extractDir }),
  }

  return {
    targets: [target],
    resolution,
  }
}

function parseScoopBin (bin: ScoopBin | undefined): string | Record<string, string> {
  if (!bin) return ''
  if (typeof bin === 'string') return bin
  const result: Record<string, string> = {}
  for (const entry of bin) {
    if (typeof entry === 'string') {
      const name = entry.replace(/\.exe$/i, '')
      result[name] = entry
    } else {
      result[entry[1]] = entry[0]
    }
  }
  return result
}

function isTarball (url: string): boolean {
  return url.endsWith('.tar.gz') || url.endsWith('.tgz') || url.endsWith('.tar.bz2')
}

async function fetchScoopManifest (
  fetchFromRegistry: FetchFromRegistry,
  name: string
): Promise<ScoopManifest> {
  for (const bucketUrl of SCOOP_BUCKETS) {
    const url = `${bucketUrl}/${encodeURIComponent(name)}.json`
    // eslint-disable-next-line no-await-in-loop
    const res = await fetchFromRegistry(url)
    if (res.ok) {
      // eslint-disable-next-line no-await-in-loop
      return await res.json() as ScoopManifest
    }
  }
  throw new PnpmError('BROOP_SCOOP_NOT_FOUND', `Package "${name}" not found in any Scoop bucket`)
}
