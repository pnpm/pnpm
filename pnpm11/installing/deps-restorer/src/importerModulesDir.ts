import path from 'node:path'

import { pathAbsolute } from 'path-absolute'

export interface ImporterModulesDirOptions {
  lockfileDir: string
  modulesDir?: string
}

/**
 * The modules directory of the importer at `importerDir`.
 *
 * The root importer's is the configured `modulesDir`, resolved against
 * `lockfileDir`. Every other project's is the same relative path inside the
 * project. An absolute `modulesDir`, or one that climbs out with `..`, cannot
 * carry over to a project, so the project gets a directory named after its
 * last segment.
 */
export function importerModulesDir (importerDir: string, opts: ImporterModulesDirOptions): string {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = pathAbsolute(modulesDir, opts.lockfileDir)
  if (path.resolve(importerDir) === path.resolve(opts.lockfileDir)) return rootModulesDir
  const relative = path.normalize(modulesDir)
  const staysInProject = !path.isAbsolute(relative) && relative !== '.' && !relative.split(path.sep).includes('..')
  return path.join(importerDir, staysInProject ? relative : path.basename(rootModulesDir))
}
