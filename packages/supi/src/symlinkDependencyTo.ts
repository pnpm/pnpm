import { linkLogger } from '@pnpm/core-loggers'
import path = require('path')
import symlinkDir = require('symlink-dir')

export default function symlinkDependencyTo (alias: string, peripheralLocation: string, dest: string, shrinkwrapDirectory: string) {
  const linkPath = path.join(dest, alias)
  // Don't symlink deps from packages that are outside of the monorepo root.
  //   These may be symlinks from another project, and we don't want to break the other project.
  if (!linkPath.startsWith(shrinkwrapDirectory)) {
    return { reused: true }
  }
  linkLogger.debug({ target: peripheralLocation, link: linkPath })
  return symlinkDir(peripheralLocation, linkPath)
}
