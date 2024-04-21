import semver from 'semver'

export function resolveWorkspaceRange (range: string, versions: string[]): string | null {
  if (range === '*' || range === '^' || range === '~') {
    return semver.maxSatisfying(versions, '*', {
      includePrerelease: true,
    })
  }
  return semver.maxSatisfying(versions, range, {
    loose: true,
  })
}
