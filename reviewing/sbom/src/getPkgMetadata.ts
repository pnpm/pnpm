import { type PackageManifest, type Registries } from '@pnpm/types'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import { readPackageJson } from '@pnpm/read-package-json'
import { type PackageSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import pLimit from 'p-limit'

const limitMetadataReads = pLimit(4)

export interface PkgMetadata {
  license?: string
  description?: string
  author?: string
  homepage?: string
  repository?: string
}

export interface GetPkgMetadataOptions {
  storeDir: string
  lockfileDir: string
  virtualStoreDirMaxLength: number
}

export async function getPkgMetadata (
  depPath: string,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  return limitMetadataReads(() => getPkgMetadataUnclamped(depPath, snapshot, registries, opts))
}

async function getPkgMetadataUnclamped (
  depPath: string,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  const id = snapshot.id ?? depPath
  const resolution = pkgSnapshotToResolution(depPath, snapshot, registries)

  let files: Map<string, string>
  try {
    const result = await readPackageFileMap(resolution, id, opts)
    if (!result) return {}
    files = result
  } catch {
    return {}
  }

  const manifestPath = files.get('package.json')
  if (!manifestPath) return {}
  const manifest = await readPackageJson(manifestPath)
  return extractMetadata(manifest)
}

function extractMetadata (manifest: PackageManifest): PkgMetadata {
  return {
    license: parseLicenseField(manifest.license),
    description: manifest.description,
    author: parseAuthorField(manifest.author),
    homepage: manifest.homepage,
    repository: parseRepositoryField(manifest.repository),
  }
}

function parseLicenseField (field: unknown): string | undefined {
  if (typeof field === 'string') return field
  if (field && typeof field === 'object' && 'type' in field) {
    return (field as { type: string }).type
  }
  if (Array.isArray(field)) {
    return field
      .map((l: { type?: string }) => l.type)
      .filter(Boolean)
      .join(' OR ') || undefined
  }
  return undefined
}

function parseAuthorField (field: unknown): string | undefined {
  if (!field) return undefined
  if (typeof field === 'string') return field
  if (typeof field === 'object' && 'name' in field) {
    return (field as { name: string }).name
  }
  return undefined
}

function parseRepositoryField (field: unknown): string | undefined {
  if (!field) return undefined
  if (typeof field === 'string') return field
  if (typeof field === 'object' && 'url' in field) {
    return (field as { url: string }).url
  }
  return undefined
}
