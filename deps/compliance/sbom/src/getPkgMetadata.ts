import { isSpdxLicenseExpression, resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { packageIdFromSnapshot, type PackageSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { DepPath, PackageManifest, Registries } from '@pnpm/types'
import pLimit from 'p-limit'

const limitMetadataReads = pLimit(4)

export interface PkgMetadata {
  license?: string
  description?: string
  author?: string
  homepage?: string
  repository?: string
  bugsUrl?: string
}

export interface GetPkgMetadataOptions {
  storeDir: string
  storeIndex: StoreIndex
  lockfileDir: string
  virtualStoreDirMaxLength: number
}

export async function getPkgMetadata (
  depPath: DepPath,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  return limitMetadataReads(() => getPkgMetadataUnclamped(depPath, snapshot, registries, opts))
}

async function getPkgMetadataUnclamped (
  depPath: DepPath,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  const id = packageIdFromSnapshot(depPath, snapshot)
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
    bugsUrl: bugsUrlFromField(manifest.bugs),
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

// `bugs` may be a URL string, a bare email, or `{ url, email }`. The CycloneDX
// issue-tracker reference expects a URL, so parse the candidate and keep it only
// when it is a well-formed http(s) URL — dropping email-only bug contacts and
// malformed values like "https://". Exported so the command's root-package
// handling uses the same rule.
export function bugsUrlFromField (field: unknown): string | undefined {
  let candidate: string | undefined
  if (typeof field === 'string') {
    candidate = field.trim()
  } else if (field && typeof field === 'object' && 'url' in field) {
    const value = (field as { url?: unknown }).url
    if (typeof value === 'string') candidate = value.trim()
  }
  if (!candidate) return undefined
  let parsed: URL
  try {
    parsed = new URL(candidate)
  } catch {
    return undefined
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined
  // Drop any embedded credentials: an SBOM is a shareable/published artifact,
  // so a `bugs` URL like `https://user:token@tracker/...` must not leak the
  // secret into externalReferences[].url. The tracker URL itself is still useful.
  parsed.username = ''
  parsed.password = ''
  // Emit the normalized URL, not the raw input: `new URL` strips CR/LF/tab and
  // percent-encodes spaces and control characters, so a crafted `bugs` value
  // can't push raw whitespace or control chars into the CycloneDX
  // `externalReferences[].url` (whose format is an `iri-reference`).
  return parsed.href
}
