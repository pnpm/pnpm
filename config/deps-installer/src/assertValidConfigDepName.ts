import { PnpmError } from '@pnpm/error'
import validateNpmPackageName from 'validate-npm-package-name'

// A config-dependency name becomes a directory pnpm creates during install
// (`node_modules/.pnpm-config/<name>`) and a store path segment, so it must be
// a valid npm package name. A traversal-shaped name (`../../PWNED`), a reserved
// name (`.bin`, `.pnpm`, `node_modules`), or `__proto__` is rejected before any
// path is built from it. Matches the `validForOldPackages` check pnpm applies
// to dependency aliases read from a manifest.
export function assertValidConfigDepName (name: string): void {
  if (!validateNpmPackageName(name).validForOldPackages) {
    throw new PnpmError(
      'INVALID_DEPENDENCY_NAME',
      `The configDependencies in pnpm-workspace.yaml contains a dependency with an invalid name: ${JSON.stringify(name)}`,
      {
        hint: 'A dependency name must be a valid npm package name — a single `name` or `@scope/name` consisting of URL-friendly characters, with no leading `.` or `_`, and not equal to reserved names such as `node_modules`.',
      }
    )
  }
}
