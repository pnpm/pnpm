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

// `repository` may be a URL, an npm hosted shorthand, an scp-style git remote,
// or `{ type, url }`. CycloneDX's `externalReferences[].url` is an
// `iri-reference`, so a raw shorthand or remote fails schema validation in
// consumers such as Dependency-Track. Absolute URLs are emitted in their
// normalized form, a value naming a repository on a host hosted-git-info knows
// is expanded to the `git+https` URL npm derives, and everything else is
// dropped. Exported so the command's root-package handling uses the same rule.
export function repositoryFromField (field: unknown): string | undefined {
  const raw = urlFieldValue(field)
  if (!raw) return undefined
  const absolute = absoluteUrl(raw)
  if (absolute) return absolute
  const hosted = HostedGit.fromUrl(raw)
  // A shorthand that names no owner (`github:repo`) still parses, but the URL
  // derived from it names `null` as the owner and points at nothing. This also
  // keeps out the ownerless `gist:<id>` shorthand, which pnpm v12's parser does
  // not know at all; dropping it in both is what keeps the two versions
  // publishing the same SBOM. A gist named by its URL is published like any
  // other URL.
  if (!hosted?.user) return undefined
  const expanded = hosted.https()
  return expanded ? urlWithoutCredentials(expanded)?.href : undefined
}

// The value as an absolute URL, or undefined when it is not one. A URL with no
// host is not: `github:owner/repo` parses, but the repository the shorthand
// names says more than the literal text does, and `mailto:` and `file:` values
// name no repository an SBOM consumer can reach.
function absoluteUrl (raw: string): string | undefined {
  const url = urlWithoutCredentials(raw)
  if (!url || url.host === '') return undefined
  return url.href
}

// `bugs` may be a URL string, a bare email, or `{ url, email }`. The CycloneDX
// issue-tracker reference expects a URL, so keep the candidate only when it is
// a well-formed http(s) URL — dropping email-only bug contacts and malformed
// values like "https://". Exported so the command's root-package handling uses
// the same rule.
export function bugsUrlFromField (field: unknown): string | undefined {
  const raw = urlFieldValue(field)
  if (!raw) return undefined
  const url = urlWithoutCredentials(raw)
  if (!url) return undefined
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined
  return url.href
}

// The string a `repository`- or `bugs`-shaped field holds: the field itself,
// or the `url` of its object form.
function urlFieldValue (field: unknown): string | undefined {
  let value: unknown = field
  if (field && typeof field === 'object' && 'url' in field) {
    value = (field as { url?: unknown }).url
  }
  if (typeof value !== 'string') return undefined
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

// An absolute URL validated and normalized by the WHATWG parser, with the
// userinfo an SBOM must not publish removed, or undefined when the value does
// not parse. Parsing is what makes the emitted form a valid iri-reference: the
// serialized URL percent-encodes the whitespace and control characters a raw
// passthrough would publish, and query or fragment text can never be mistaken
// for userinfo.
function urlWithoutCredentials (raw: string): URL | undefined {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return undefined
  }
  // An ssh remote addresses its host as `git@github.com`, so a username with
  // no password there names the login, not a secret. Under any other scheme
  // the username alone can be the secret: GitHub and GitLab both take a token
  // in place of the whole `user:password`.
  const sshLogin = !url.password && (url.protocol === 'ssh:' || url.protocol === 'git+ssh:')
  if (!sshLogin) {
    url.username = ''
    url.password = ''
  }
  return url
}
