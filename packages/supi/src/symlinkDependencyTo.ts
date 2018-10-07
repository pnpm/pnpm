import { linkLogger } from '@pnpm/core-loggers'
import path = require('path')
import symlinkDir = require('symlink-dir')

export default function symlinkDependencyTo (alias: string, peripheralLocation: string, dest: string) {
  const linkPath = path.join(dest, alias)
  linkLogger.debug({ target: peripheralLocation, link: linkPath })
  return symlinkDir(peripheralLocation, linkPath)
}
