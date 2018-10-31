import { linkLogger } from '@pnpm/core-loggers'
import path = require('path')
import symlinkDir = require('symlink-dir')

export default function symlinkDependencyTo (alias: string, peripheralLocation: string, dest: string) {
  const linkPath = path.join(dest, alias)
  // Don't symlink deps from packages that are outside of the monorepo root.
  //   These may be symlinks from another project, and we don't want to break the other project.
  // TODO(vjpr): Is `process.cwd()` a reliable way to get the monorepo root?
  if (!linkPath.startsWith(process.cwd())) {
    // TODO(vjpr): Should print some kind of warning maybe?
    return {reused: true} // TODO(vjpr): Not sure we should misuse reused like this.
  }
  linkLogger.debug({ target: peripheralLocation, link: linkPath })
  return symlinkDir(peripheralLocation, linkPath)
}
