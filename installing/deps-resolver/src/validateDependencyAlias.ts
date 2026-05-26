import { PnpmError } from '@pnpm/error'
import validateNpmPackageName from 'validate-npm-package-name'

// An alias is the directory name pnpm creates inside `node_modules`, so
// it must be a valid npm package name. Anything else (path-traversal
// shapes such as `@x/../../../../../.git/hooks`, control characters,
// names that collide with pnpm's own `node_modules` layout such as
// `.bin` / `.pnpm` / `node_modules`) is rejected. Matches the
// `validForOldPackages` check `parseWantedDependency` applies to
// CLI-given names.
export function isValidDependencyAlias (alias: string): boolean {
  return typeof alias === 'string' && validateNpmPackageName(alias).validForOldPackages
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
          hint: 'A dependency name must be a valid npm package name — a single `name` or `@scope/name` consisting of URL-friendly characters, with no leading `.` or `_`, and not equal to reserved names such as `node_modules`.',
        }
      )
    }
  }
}
