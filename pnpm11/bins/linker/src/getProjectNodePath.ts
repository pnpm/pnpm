import { promises as fs } from 'node:fs'
import path from 'node:path'

/**
 * Returns the project's modules directory for command shims when
 * `extendNodePath` is enabled. Keeping the project directory explicit makes
 * bin resolution stable when the target package is reached through a
 * symlinked or junction store path. Paths containing the path-list delimiter
 * cannot be represented as a single `NODE_PATH` entry and are omitted.
 * `includeDefault` is used by install-time bin linking; lifecycle environments
 * keep the default `node_modules` directory implicit.
 */
export async function getProjectNodePath (
  project: { modulesDir: string, rootDir: string },
  opts: { extendNodePath?: boolean, includeDefault?: boolean }
): Promise<string | undefined> {
  if (opts.extendNodePath === false || project.modulesDir.includes(path.delimiter) || opts.includeDefault !== true && await isSameDir(project.modulesDir, path.join(project.rootDir, 'node_modules'))) {
    return undefined
  }
  return project.modulesDir
}

async function isSameDir (a: string, b: string): Promise<boolean> {
  return a === b || await realpathOrSelf(a) === await realpathOrSelf(b)
}

async function realpathOrSelf (dir: string): Promise<string> {
  try {
    return await fs.realpath(dir)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return dir
  }
}
