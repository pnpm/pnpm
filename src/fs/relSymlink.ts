import symlink, {SymlinkType} from './forceSymlink'
import path = require('path')
import os = require('os')

// Always use "junctions" on Windows. Even though support for "symbolic links" was added in Vista+, users by default
// lack permission to create them
const symlinkType: SymlinkType = os.platform() === 'win32' ? 'junction' : 'dir'

/*
 * Relative symlink
 */

export default function relSymlink (src: string, dest: string) {
  // Junction points can't be relative
  const rel = symlinkType !== 'junction' ? path.relative(path.dirname(dest), src) : src

  return symlink(rel, dest, symlinkType)
}
