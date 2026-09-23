import semver from 'semver'

/**
 * Picks the highest workspace version matching `range`.
 *
 * Versions that are not valid semver, such as `1` or `1.0`, still match the
 * wildcard tokens (`*`, `^`, `~`, and the empty string) when no semver version
 * is present, and match any other range only when it is identical to them.
 */
export function resolveWorkspaceRange (range: string, versions: string[]): string | null {
  if (range === '*' || range === '^' || range === '~' || range === '') {
    return semver.maxSatisfying(versions, '*', {
      includePrerelease: true,
    }) ?? ([...versions].sort().at(-1) ?? null)
  }
  return semver.maxSatisfying(versions, range, {
    loose: true,
  }) ?? (versions.includes(range) ? range : null)
}

