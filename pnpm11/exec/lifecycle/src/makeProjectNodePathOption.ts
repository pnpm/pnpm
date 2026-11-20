import path from 'node:path'

import { getProjectNodePath } from '@pnpm/bins.linker'
import isWindows from 'is-windows'

/**
 * The `NODE_PATH` for the scripts and commands of a project whose executables
 * are symlinks. A symlink has no shim to put the project's custom modules
 * directory on `NODE_PATH`, so the environment carries it, ahead of the rest.
 */
export async function makeProjectNodePathOption (
  project: { modulesDir: string, rootDir: string },
  opts: { extendNodePath?: boolean, extraEnv?: Record<string, string | undefined>, preferSymlinkedExecutables?: boolean }
): Promise<Record<string, string>> {
  if (!opts.preferSymlinkedExecutables || isWindows()) return {}
  const projectNodePath = await getProjectNodePath(project, opts)
  if (projectNodePath == null) return {}
  const rest = (opts.extraEnv?.NODE_PATH ?? process.env.NODE_PATH ?? '')
    .split(path.delimiter)
    .filter((entry) => entry !== '' && entry !== projectNodePath)
  return { NODE_PATH: [projectNodePath, ...rest].join(path.delimiter) }
}
