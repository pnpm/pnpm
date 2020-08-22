import { linkLogger } from '@pnpm/core-loggers'

import symlinkDirectRootDependency from './symlinkDirectRootDependency'
import path = require('path')
import symlinkDir = require('symlink-dir')

export default function symlinkDependency (
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
) {
  const link = path.join(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  return symlinkDir(dependencyRealLocation, link)
}

export { symlinkDirectRootDependency }
