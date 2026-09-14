import { isSpdxLicenseExpression, resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { packageIdFromSnapshot, type PackageSnapshot, pkgSnapshotToResolution, type PkgSnapshotToResolutionOptions } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { DepPath, PackageManifest } from '@pnpm/types'
import HostedGit from 'hosted-git-info'
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
  registryOpts: PkgSnapshotToResolutionOptions,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  return limitMetadataReads(() => getPkgMetadataUnclamped(depPath, snapshot, registryOpts, opts))
}

async function getPkgMetadataUnclamped (
  depPath: DepPath,
  snapshot: PackageSnapshot,
  registryOpts: PkgSnapshotToResolutionOptions,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  const id = packageIdFromSnapshot(depPath, snapshot)
  const resolution = pkgSnapshotToResolution(depPath, snapshot, registryOpts)

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
    author: authorNameFromField(manifest.author),
    homepage: manifest.homepage,
    repository: repositoryFromField(manifest.repository),
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

// `author` may be a string or `{ name, email, url }`. A blank name names
// nobody, so it reads as no author at all: SPDX would otherwise emit the
// nameless actor `Person: `, which strict consumers reject. Exported so the
// command's root-package and workspace-package handling uses the same rule.
export function authorNameFromField (field: unknown): string | undefined {
  let name: unknown
  if (typeof field === 'string') {
    name = field
  } else if (field && typeof field === 'object' && 'name' in field) {
    name = (field as { name?: unknown }).name
  }
  if (typeof name !== 'string' || !name.trim()) return undefined
  return name
}

// `repository` may be a URL, an npm shorthand, an scp-style git remote, or
// `{ type, url }`. CycloneDX requires an `iri-reference`, so a raw shorthand or
// remote fails validation in consumers such as Dependency-Track. Exported so
// the command's root-package handling uses the same rule.
export function repositoryFromField (field: unknown): string | undefined {
  const raw = urlFieldValue(field)
  if (!raw) return undefined
  const absolute = absoluteUrl(raw)
  if (absolute) return absolute
  const hosted = HostedGit.fromUrl(raw)
  // An ownerless shorthand derives a URL naming `null` as the owner.
  // `gist:<id>` is ownerless too and pnpm v12's parser has no gist support, so
  // dropping it here is what keeps the two versions in step.
  if (!hosted?.user) return undefined
  const expanded = hosted.https()
  return expanded ? urlWithoutCredentials(expanded)?.href : undefined
}

// The value as an absolute URL. One with no host is not: `github:owner/repo`
// belongs to the shorthand parser, and `mailto:` and `file:` values name no
// repository a consumer can reach.
function absoluteUrl (raw: string): string | undefined {
  const url = urlWithoutCredentials(raw)
  if (!url || url.host === '') return undefined
  return url.href
}

// `bugs` may be a URL string, a bare email, or `{ url, email }`. Only a
// well-formed http(s) URL becomes the CycloneDX issue-tracker reference.
// Exported so the command's root-package handling uses the same rule.
export function bugsUrlFromField (field: unknown): string | undefined {
  const raw = urlFieldValue(field)
  if (!raw) return undefined
  const url = urlWithoutCredentials(raw)
  if (!url) return undefined
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined
  return url.href
}

function urlFieldValue (field: unknown): string | undefined {
  let value: unknown = field
  if (field && typeof field === 'object' && 'url' in field) {
    value = (field as { url?: unknown }).url
  }
  if (typeof value !== 'string') return undefined
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

// An absolute URL in the WHATWG parser's normalized form, without the userinfo
// an SBOM must not publish. Parsing is most of what makes the result a valid
// iri-reference: it percent-encodes whitespace and control characters, and
// query text can never be mistaken for userinfo.
function urlWithoutCredentials (raw: string): URL | undefined {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return undefined
  }
  // The parser keeps a `%` that begins no `%XX` escape as the manifest wrote
  // it, and an iri-reference admits no such thing.
  if (/%(?![0-9a-f]{2})/i.test(url.href)) return undefined
  // `ssh:` and `git+ssh:` address their host as `git@github.com`, so a
  // username with no password is part of the address there. Under any other
  // scheme it can be the secret itself: GitHub and GitLab take a token in
  // place of the whole `user:password`.
  const sshLogin = !url.password && (url.protocol === 'ssh:' || url.protocol === 'git+ssh:')
  if (!sshLogin) {
    url.username = ''
    url.password = ''
  }
  return url
}
