export interface Packument {
  'dist-tags': Record<string, string>
  versions: Record<string, unknown>
}

/**
 * Turns what the user asked for into a concrete pnpm version.
 *
 * `spec` may be a dist-tag (`latest`, `next-12`), an exact version (`11.20.0`,
 * with an optional leading `v`), or a bare major (`12`), which picks that
 * major's stable release and falls back to its prerelease lane.
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

  throw new Error(`Sorry! pnpm version "${spec}" could not be found. Available tags: ${Object.keys(distTags).sort().join(', ')}`)
}
