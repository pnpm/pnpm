import path from 'node:path'

import { pathAbsolute } from 'path-absolute'

/**
 * A project's modules directory.
 *
 * `modulesDir` may be relative to `projectDir` or absolute, and is `undefined`
 * when the project uses the default `node_modules`.
 */
export function modulesDirOf (projectDir: string, modulesDir: string | undefined): string {
  return pathAbsolute(modulesDir ?? 'node_modules', projectDir)
}

/**
 * The directory holding a project's executables. `modulesDir` is as for
 * {@link modulesDirOf}.
 */
export function binDirOf (projectDir: string, modulesDir: string | undefined): string {
  return path.join(modulesDirOf(projectDir, modulesDir), '.bin')
}
