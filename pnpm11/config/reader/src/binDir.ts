import path from 'node:path'

import { pathAbsolute } from 'path-absolute'

/**
 * The directory holding a project's executables.
 *
 * `modulesDir` may be relative to `projectDir` or absolute, and is `undefined`
 * when the project uses the default `node_modules`.
 */
export function binDirOf (projectDir: string, modulesDir: string | undefined): string {
  return path.join(pathAbsolute(modulesDir ?? 'node_modules', projectDir), '.bin')
}
