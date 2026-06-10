import { PnpmError } from '@pnpm/error'
import { type PackageSnapshot } from '@pnpm/lockfile.types'
import * as dp from '@pnpm/dependency-path'

/**
 * Matches the known git-host tarball download endpoints. Schemes and
 * hostnames are case-insensitive, so the URL is matched against a lowercased
 * copy: a tampered `https://CODELOAD.GITHUB.COM/...` must not slip past as a
 * non-git-hosted (and therefore registry-trusted) tarball. Only the
 * lowercased copy is inspected; the original URL is never rewritten.
 */
export function isGitHostedTarballUrl (url: string): boolean {
  const lowerUrl = url.toLowerCase()
  return (
    lowerUrl.startsWith('https://codeload.github.com/') ||
    lowerUrl.startsWith('https://bitbucket.org/') ||
    lowerUrl.startsWith('https://gitlab.com/')
  ) && lowerUrl.includes('tar.gz')
}

/**
 * A registry-style depPath (name@semver) must be backed by a registry-shaped
 * resolution: the allowBuild policy derives a trusted package identity from
 * that key shape, which is only sound while this invariant holds. The check
 * is offline and runs wherever a lockfile entry is materialized into a
 * fetchable resolution or its scripts are about to run.
 */
export function assertRegistryShapedResolution (depPath: string, pkgSnapshot: PackageSnapshot): void {
  const { name, version, nonSemverVersion } = dp.parse(depPath)
  if (name == null || version == null || nonSemverVersion != null) return
  if (isRegistryShapedResolution(pkgSnapshot.resolution)) return
  throw new PnpmError('RESOLUTION_SHAPE_MISMATCH',
    `Cannot use the lockfile entry of "${depPath}": its registry-style dependency path is backed by a non-registry resolution.`,
    { hint: 'The lockfile may be corrupted or have been tampered with. Restore it from a trusted source, or delete it and re-run installation without --frozen-lockfile to regenerate.' }
  )
}

function isRegistryShapedResolution (resolution: unknown): boolean {
  if (resolution == null) return true
  if (typeof resolution !== 'object') return false
  const { type, gitHosted, tarball, variants } = resolution as {
    type?: unknown
    gitHosted?: unknown
    tarball?: unknown
    variants?: unknown
  }
  if (type === 'variations') {
    return Array.isArray(variants) && variants.every(
      (variant) => isRegistryShapedResolution((variant as { resolution?: unknown })?.resolution)
    )
  }
  if (type != null) return false
  // Plain tarball / registry resolution. The lockfile is parsed from YAML
  // without schema validation, so the `gitHosted` flag is not trustworthy on
  // its own: a tampered entry could set a non-boolean (dodging a strict
  // `=== true`) or an explicit `false` on a git-host URL (the loader only
  // backfills the flag when absent). Treat any non-boolean flag as git-hosted
  // and gate on the URL so the verdict never depends on the flag alone.
  if (gitHosted != null && (typeof gitHosted !== 'boolean' || gitHosted)) return false
  // A registry resolution reconstructs its tarball URL from name+version, so
  // an absent/empty `tarball` is registry-shaped. When a URL with a scheme is
  // present it must be an http(s) artifact: a `file:` tarball under a
  // name@semver key is a local artifact that a package-name rule must not
  // approve.
  if (typeof tarball === 'string' && tarball !== '') {
    if (hasUrlScheme(tarball)) {
      if (!/^https?:\/\//i.test(tarball)) return false
      if (isGitHostedTarballUrl(tarball)) return false
    } else if (tarball.startsWith('/') || tarball.startsWith('\\')) {
      // Protocol-relative and path-absolute forms (`//host`, `/\host`, ...)
      // can escape the configured registry host when resolved as a URL.
      return false
    }
    // A scheme-less relative path is resolved against the configured
    // registry, so it cannot point off-registry.
  }
  return true
}

function hasUrlScheme (url: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(url)
}
