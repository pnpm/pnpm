import path from 'path'
import { linkLogger } from '@pnpm/core-loggers'

import symlinkDir from 'symlink-dir'
import symlinkDirectRootDependency from './symlinkDirectRootDependency'

export async function symlinkDependency (
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
) {
  const link = path.join(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  return symlinkDir(dependencyRealLocation, link)
}

export { symlinkDirectRootDependency }
