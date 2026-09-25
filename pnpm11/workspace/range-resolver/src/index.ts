import semver from 'semver'

/**
 * Picks the highest workspace version matching `range`.
 *
 * A version identical to `range` wins over semver matches, so a saved
 * `workspace:1` keeps pointing at a project whose version is `1` even when
 * another copy is at `1.2.3`. Versions that are not valid semver, such as `1`
 * or `1.0`, also match the wildcard tokens (`*`, `^`, `~`, and the empty
 * string) when no semver version is present.
 */
export function resolveWorkspaceRange (range: string, versions: string[]): string | null {
  if (range === '*' || range === '^' || range === '~' || range === '') {
    return semver.maxSatisfying(versions, '*', {
      includePrerelease: true,
    }) ?? versions.reduce<string | null>((max, version) => max == null || version > max ? version : max, null)
  }
  if (versions.includes(range)) return range
  return semver.maxSatisfying(versions, range, {
    loose: true,
  })
}
