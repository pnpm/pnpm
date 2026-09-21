import { promises as fs } from 'node:fs'
import path from 'node:path'

/**
 * Returns the `NODE_PATH` entry for the bins of a project's direct
 * dependencies: the project's modules directory when it is not
 * `<project>/node_modules`, or `undefined`. Node finds a project's packages
 * only by walking up to `node_modules` directories, so tools that load plugins
 * relative to the project need this entry to find them in a custom modules
 * directory.
 */
export async function getProjectNodePath (
  project: { modulesDir: string, rootDir: string },
  opts: { extendNodePath?: boolean }
): Promise<string | undefined> {
  if (opts.extendNodePath === false || project.modulesDir === await realpathOrSelf(path.join(project.rootDir, 'node_modules'))) {
    return undefined
  }
  return project.modulesDir
}

async function realpathOrSelf (dir: string): Promise<string> {
  try {
    return await fs.realpath(dir)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return dir
  }
}
