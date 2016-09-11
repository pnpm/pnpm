import fs = require('mz/fs')
import createDebug from '../debug'
const debug = createDebug('pnpm:symlink')

export type SymlinkType = 'junction' | 'dir'

/*
 * Creates a symlink. Re-link if a symlink already exists at the supplied
 * srcPath. API compatible with [`fs#symlink`](https://nodejs.org/api/fs.html#fs_fs_symlink_srcpath_dstpath_type_callback).
 */

export default function forceSymlink (srcPath: string, dstPath: string, type: SymlinkType) {
  debug(`${srcPath} -> ${dstPath}`)
  try {
    fs.symlinkSync(srcPath, dstPath, type)
    return Promise.resolve()
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'EEXIST') return Promise.reject(err)

    return fs.readlink(dstPath)
      .then((linkString: string) => {
        if (srcPath === linkString) {
          return Promise.resolve()
        }
        return fs.unlink(dstPath)
          .then(() => forceSymlink(srcPath, dstPath, type))
      })
  }
}
