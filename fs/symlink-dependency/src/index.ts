import { linkLogger } from '@pnpm/core-loggers'
import { symlinkDir, symlinkDirSync } from 'symlink-dir'

import { safeJoinModulesDir } from './safeJoinModulesDir.js'

export { safeJoinModulesDir } from './safeJoinModulesDir.js'
export { symlinkDirectRootDependency } from './symlinkDirectRootDependency.js'

export async function symlinkDependency (
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
): Promise<{ reused: boolean, warn?: string }> {
  const link = safeJoinModulesDir(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
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
