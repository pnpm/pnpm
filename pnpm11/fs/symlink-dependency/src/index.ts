import { linkLogger } from '@pnpm/core-loggers'
import { symlinkDir, symlinkDirSync } from 'symlink-dir'

import { forceAbsoluteSymlink } from './forceAbsoluteSymlink.js'
import { safeJoinModulesDir } from './safeJoinModulesDir.js'

export { forceAbsoluteSymlink } from './forceAbsoluteSymlink.js'
export { safeJoinModulesDir } from './safeJoinModulesDir.js'
export { symlinkDirectRootDependency } from './symlinkDirectRootDependency.js'

export async function symlinkDependency (
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string,
  opts?: { absolute?: boolean }
): Promise<{ reused: boolean, warn?: string }> {
  const link = safeJoinModulesDir(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  if (opts?.absolute) {
    return forceAbsoluteSymlink(dependencyRealLocation, link)
  }
  return symlinkDir(dependencyRealLocation, link)
}

export function symlinkDependencySync (
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
): { reused: boolean, warn?: string } {
  const link = safeJoinModulesDir(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  return symlinkDirSync(dependencyRealLocation, link)
}
