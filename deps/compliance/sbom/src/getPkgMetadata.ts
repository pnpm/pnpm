import { isSpdxLicenseExpression, resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { type PackageSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { PackageManifest, Registries } from '@pnpm/types'
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
  storeIndex: StoreIndex
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
  return extractMetadata(manifest, files)
}

async function extractMetadata (manifest: PackageManifest, files: Map<string, string>): Promise<PkgMetadata> {
  const license = await resolveLicense({ manifest, files })
  return {
    license: serializableLicense(license),
    description: manifest.description,
    author: parseAuthorField(manifest.author),
    homepage: manifest.homepage,
    repository: parseRepositoryField(manifest.repository),
  }
}

// Drop:
//   - missing / "Unknown" — so serializers emit NOASSERTION / absence.
//   - LICENSE-file-detected values that aren't SPDX-valid (e.g. "Eclipse Public
//     License 1.0") — would produce non-compliant SPDX output. Manifest-declared
//     licenses are trusted as-is; authors use SPDX expressions there.
function serializableLicense (license: { name: string, licenseFile?: string } | undefined): string | undefined {
  if (!license || license.name === 'Unknown') return undefined
  if (license.licenseFile && !isSpdxLicenseExpression(license.name)) return undefined
  return license.name
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
