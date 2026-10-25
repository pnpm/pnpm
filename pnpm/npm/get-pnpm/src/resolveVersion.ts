import semver from 'semver'

export interface Packument {
  'dist-tags': Record<string, string>
  versions: Record<string, unknown>
}

/**
 * Turns what the user asked for into a concrete pnpm version.
 *
 * `spec` may be a dist-tag (`latest`, `next-12`), an exact version (`11.20.0`,
 * with an optional leading `v`), a bare major (`12`), which picks that major's
 * stable release and falls back to its prerelease lane, or a semver range
 * (`^12.1.0`, `>=11 <13`).
 *
 * Dist-tags win over exact versions, matching `install.ps1`.
 *
 * @throws if `spec` matches neither a dist-tag nor a published version.
 */
export function resolveVersion (packument: Packument, spec: string): string {
  const distTags = packument['dist-tags']
  if (distTags[spec]) return distTags[spec]

  const version = spec.startsWith('v') ? spec.slice(1) : spec
  if (packument.versions[version]) return version

  if (/^\d+$/.test(version)) {
    const majorTag = distTags[`latest-${version}`] ?? distTags[`next-${version}`]
    if (majorTag) return majorTag
  }

  const inRange = semver.validRange(spec) == null ? undefined : versionInRange(packument, spec)
  if (inRange != null) return inRange

  throw new Error(`Sorry! pnpm version "${spec}" could not be found. Available tags: ${Object.keys(distTags).sort().join(', ')}`)
}

/**
 * The version npm would install for `range`: the `latest` release when it is
 * in range, otherwise the highest that is. A prerelease is picked only by a
 * range that names one, as in npm and in pnpm's switch to a project's pinned
 * version.
 */
function versionInRange (packument: Packument, range: string): string | undefined {
  const latest = packument['dist-tags'].latest
  if (latest != null && semver.satisfies(latest, range)) return latest
  return semver.maxSatisfying(Object.keys(packument.versions), range) ?? undefined
}

/** The major of an exact version, as the package layout depends on it. */
export function majorVersion (version: string): number {
  const field = version.split('.')[0] ?? ''
  // Digits only: `Number` reads '' as 0 and '0x10' as 16, and a caller-supplied
  // version that means neither should say so here rather than fail later as a
  // package name nobody publishes.
  if (!/^\d+$/.test(field)) {
    throw new Error(`Could not read a major version from "${version}".`)
  }
  return Number(field)
}
