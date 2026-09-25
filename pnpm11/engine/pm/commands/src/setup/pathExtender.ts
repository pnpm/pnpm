import {
  addDirToEnvPath as addDirToEnvPathExt,
  type AddDirToEnvPathOpts,
  type ConfigReport,
  type PathExtenderReport,
} from '@pnpm/os.env.path-extender'

import { addDirToPosixEnvPath } from './pathExtenderPosix.js'

export type { ConfigReport, PathExtenderReport }

export async function addDirToEnvPath (
  dir: string,
  opts: AddDirToEnvPathOpts
): Promise<PathExtenderReport> {
  if (process.platform === 'win32') {
    return addDirToEnvPathExt(dir, opts)
  }
  return addDirToPosixEnvPath(dir, opts)
}
