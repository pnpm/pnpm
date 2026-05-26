import { PnpmError } from '@pnpm/error'

// Aliases are joined with `node_modules` paths during install. An alias
// containing `..`, an embedded slash beyond a single scope separator, a
// backslash, or a null byte can escape the intended directory and let
// transitive registry metadata write symlinks at attacker-chosen paths.
// Allowed shapes are exactly `name` and `@scope/name`.
export function isValidDependencyAlias (alias: string): boolean {
  if (typeof alias !== 'string' || alias.length === 0) return false
  if (alias.includes('\0') || alias.includes('\\')) return false
  const segments = alias.split('/')
  if (segments.length > 2) return false
  for (const segment of segments) {
    if (segment === '' || segment === '.' || segment === '..') return false
  }
  if (segments.length === 2 && !segments[0].startsWith('@')) return false
  return true
}

export function assertValidDependencyAliases (
  deps: Record<string, unknown> | undefined,
  parentPkgDescription: string
): void {
  if (deps == null) return
  for (const alias of Object.keys(deps)) {
    if (!isValidDependencyAlias(alias)) {
      throw new PnpmError(
        'INVALID_DEPENDENCY_NAME',
        `${parentPkgDescription} contains a dependency with an invalid name: ${JSON.stringify(alias)}`,
        {
          hint: 'Dependency names must be a single package name or "@scope/name" — they cannot contain path-separator segments such as "..".',
        }
      )
    }
  }
}
