import symlink, {SymlinkType} from './forceSymlink'
import path = require('path')
import os = require('os')
import mkdirp from './mkdirp'
import normalize = require('normalize-path')

// Always use "junctions" on Windows. Even though support for "symbolic links" was added in Vista+, users by default
// lack permission to create them
const symlinkType: SymlinkType = os.platform() === 'win32' ? 'junction' : 'dir'

/**
 * Relative symlink
 */
export default async function relSymlink (src: string, dest: string) {
  // Junction points can't be relative
  const rel = symlinkType !== 'junction' ? path.relative(path.dirname(dest), src) : src

  try {
    await symlink(rel, dest, symlinkType)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code === 'ENOENT') {
      await mkdirp(normalize(dest).split('/').slice(0, -1).join('/'))
      await symlink(rel, dest, symlinkType)
      return
    }
  }
}
