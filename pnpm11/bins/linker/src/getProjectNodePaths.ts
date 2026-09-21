import { promises as fs } from 'node:fs'
import path from 'node:path'

/**
 * Returns the `NODE_PATH` entries for the bins of a project's direct
 * dependencies: `extraNodePaths`, followed by the project's modules directory
 * when it is not `<project>/node_modules`. Tools that load plugins relative to
 * the project walk up to `node_modules` directories, so they cannot find
 * packages installed into a custom modules directory without this entry. It
 * goes last, so the bin's own dependencies keep precedence.
 */
export async function getProjectNodePaths (
  project: { modulesDir: string, rootDir: string },
  opts: { extendNodePath?: boolean, extraNodePaths?: string[] }
): Promise<string[] | undefined> {
  if (opts.extendNodePath === false || project.modulesDir === await realpathOrSelf(path.join(project.rootDir, 'node_modules'))) {
    return opts.extraNodePaths
  }
  return [...opts.extraNodePaths ?? [], project.modulesDir]
}

async function realpathOrSelf (dir: string): Promise<string> {
  try {
    return await fs.realpath(dir)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return dir
  }
}
