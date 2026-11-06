import { pathAbsolute } from 'path-absolute'

/**
 * The modules directory of the importer at `importerDir`: the configured
 * `modulesDir`, resolved against that importer's directory, as each project's
 * context resolves it.
 */
export function importerModulesDir (importerDir: string, modulesDir: string | undefined): string {
  return pathAbsolute(modulesDir ?? 'node_modules', importerDir)
}
