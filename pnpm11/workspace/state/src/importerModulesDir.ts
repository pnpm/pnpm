import path from 'node:path'

import { pathAbsolute } from 'path-absolute'

/**
 * The modules directory of the importer at `importerDir`: the configured
 * `modulesDir`, resolved against that importer's directory, as each project's
 * context resolves it.
 *
 * A workspace project whose resolved `modulesDir` is not strictly inside it,
 * because the value is absolute or climbs out with `..`, keeps `node_modules`.
 * Otherwise every project would share one directory, and the copies of
 * conflicting versions would overwrite each other.
 */
export function importerModulesDir (
  importerDir: string,
  opts: { lockfileDir: string, modulesDir?: string }
): string {
  const modulesDir = pathAbsolute(opts.modulesDir ?? 'node_modules', importerDir)
  const isRoot = path.resolve(importerDir) === path.resolve(opts.lockfileDir)
  return isRoot || isStrictlyInside(modulesDir, importerDir)
    ? modulesDir
    : path.join(importerDir, 'node_modules')
}

export function isStrictlyInside (dir: string, parent: string): boolean {
  const relative = path.relative(parent, dir)
  return relative !== '' && !path.isAbsolute(relative) && relative.split(path.sep)[0] !== '..'
}
