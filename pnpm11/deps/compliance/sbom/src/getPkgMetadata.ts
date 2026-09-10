import { isSpdxLicenseExpression, resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { packageIdFromSnapshot, type PackageSnapshot, pkgSnapshotToResolution, type PkgSnapshotToResolutionOptions } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { DepPath, PackageManifest } from '@pnpm/types'
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

// `repository` may be a URL, the npm `owner/repo` shorthand, an scp-style git
// remote, or `{ type, url }`. CycloneDX's `externalReferences[].url` is an
// `iri-reference`, so a raw shorthand or remote fails schema validation in
// consumers such as Dependency-Track. Absolute URLs are parsed and emitted
// in their normalized form (with embedded credentials dropped), the
// shorthand is expanded to the GitHub URL npm's own hosted-git-info derives,
// and everything else is dropped. Exported so the command's root-package
// handling uses the same rule.
export function repositoryFromField (field: unknown): string | undefined {
  let raw: string | undefined
  if (typeof field === 'string') {
    raw = field.trim()
  } else if (field && typeof field === 'object' && 'url' in field) {
    const value = (field as { url?: unknown }).url
    if (typeof value === 'string') raw = value.trim()
  }
  if (!raw) return undefined
  return repositoryUrl(raw)
}

// A `repository` value safe to emit as an SBOM URL: an absolute URL in its
// normalized form, with embedded credentials stripped as for `bugs`, or the
// expanded npm `owner/repo` GitHub shorthand.
function repositoryUrl (raw: string): string | undefined {
  if (raw.includes('://')) return urlWithoutCredentials(raw)
  return githubShorthandUrl(raw)
}

// The npm `owner/repo` shorthand, which npm's hosted-git-info resolves to a
// `git+https` GitHub URL (what `normalize-package-data` and `npm view`
// derive, so an SBOM shows the URL npm itself would). A value is shorthand
// only when it has exactly two non-empty segments and no scheme marker,
// user, fragment, or whitespace.
function githubShorthandUrl (raw: string): string | undefined {
  if (raw.startsWith('.')) return undefined
  const segments = raw.split('/')
  if (segments.length !== 2 || segments.some((segment) => segment.length === 0)) return undefined
  for (const ch of raw) {
    if (ch === ':' || ch === '@' || ch === '#' || isAsciiWhitespace(ch)) return undefined
  }
  const repoPath = raw.endsWith('.git') ? raw : `${raw}.git`
  return `git+https://github.com/${repoPath}`
}

// An absolute URL validated and normalized by the WHATWG parser, with any
// `user:password` removed, or undefined when the value does not parse or the
// password cannot be removed. Parsing is what makes the emitted form a valid
// iri-reference: the serialized URL percent-encodes the whitespace and
// control characters a raw passthrough would publish, and query or fragment
// text can never be mistaken for userinfo. A bare username without a
// password is part of the URL, not a credential, and stays. An SBOM is a
// published artifact, so a URL whose password cannot be removed is dropped
// rather than published with the secret.
function urlWithoutCredentials (raw: string): string | undefined {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return undefined
  }
  if (url.password) {
    url.username = ''
    url.password = ''
    if (url.password) return undefined
  }
  return url.href
}

function isAsciiWhitespace (ch: string): boolean {
  const code = ch.charCodeAt(0)
  return code === 9 || code === 10 || code === 11 || code === 12 || code === 13 || code === 32
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
