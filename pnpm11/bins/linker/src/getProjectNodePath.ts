import path from 'node:path'

/**
 * Returns the project's modules directory for command shims when
 * `extendNodePath` is enabled. Keeping the project directory explicit makes
 * bin resolution stable when the target package is reached through a
 * symlinked or junctioned store path. Paths containing the path-list delimiter
 * cannot be represented as a single `NODE_PATH` entry and are omitted.
 */
export async function getProjectNodePath (
  project: { modulesDir: string, rootDir: string },
  opts: { extendNodePath?: boolean }
): Promise<string | undefined> {
  if (opts.extendNodePath === false || project.modulesDir.includes(path.delimiter)) {
    return undefined
  }
  return project.modulesDir
}
